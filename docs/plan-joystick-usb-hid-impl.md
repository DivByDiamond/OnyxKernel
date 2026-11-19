# USB HID-джойстик: план реализации

> **Для агентов:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (рекомендуется) или superpowers:executing-plans. Шаги — чекбоксы
> (`- [ ]`), отмечать по ходу.

**Goal:** Onyx сама перечисляет USB HID-джойстик (T.16000M) и отдаёт
его оси/кнопки в userspace через `/dev/js0`, стик двигает курсор.

**Architecture:** поверх существующих HCD (xHCI для QEMU, OHCI/EHCI
для SG2000) строится enumeration-core с реестром устройств-слотов,
interrupt-IN, HID-парсер и devfs-узел. Потребляет существующий
`usb::control_transfer`; закрывает TODO(2026-11-09) в
`drivers/bus/usb/hcd.rs`.

**Tech Stack:** Rust no_std (ядро), C99 (приложение `jstest`), QEMU
`qemu-xhci` + `usb-kbd`/`usb-tablet` для smoke.

**Spec:** `docs/plan-joystick-usb-hid.md` (утверждён 2026-11-19).
Спорк не создаётся: изменения коммитятся в **своих** репозиториях
(OnyxKernel / OnyxApps / OnyxOS), git — только внутри компонента.

## Global Constraints

- ≤250 строк на файл; тесты в `foo/tests.rs` (подпапка), в файле кода —
  только `#[cfg(test)] mod tests;`.
- `unwrap`/`expect`/`panic!` запрещены на путях ввода «с провода»
  (descriptor'ы, HID-репорты) — только `KResult`/`Result`.
- Все USB/HID-коды — именованные константы; `scause`-правило
  аналогично: неизвестный тип элемента — явная ветка ошибки.
- Мутации USB-состояния — под SpinLock с SIE=0; poll-модель без IRQ
  (вписаться в тик `srv/event.rs`, 100 Hz).
- Каждый `unsafe fn` — `# Safety`, каждый `unsafe {}` — `// SAFETY:`.
- Коммиты: `feat:`/`fix:`/`docs:` внутри репозитория компонента;
  каждый таск = сфокусированный коммит.
- Host-тесты: `cargo test -p onyx_kernel` (ядровые тесты гоняются на
  хосте); fmt/Clippy строгие (`-D warnings`), без `#[allow(dead_code)]`.

## Review Focus

1. **Truncated/битый descriptor с устройства** → `enumerate` возвращает
   `Err`, слот реестра не занимается и не течёт; паники нет.
   Тест: таск 3 (фикстуры битых descriptor'ов).
2. **Stall/таймаут control-передачи посреди enumeration** → `Err`
   наружу, частично инициализированное устройство не остаётся в
   реестре. Тест: таск 3 (фейк-шина отвечает ошибкой на N-м шаге).
3. **HID report descriptor с полями сверх ёмкости таблицы** →
   безопасный отказ (`Err(HidError::TooManyFields)`), не тихое
   отсечение. Тест: таск 6.
4. **Пустое кольцо `/dev/jsN`** → `read` = `Errno::Again`, не
   `Ok(0)` (EOF). Тест: таск 9.
5. **Claim-жизненный цикл** → open глушит маппер курсора, close
   возвращает; за время claim'а в кольцо копятся события, маппер не
   двигает курсор. Тест: таск 8 (claim) + таск 9 (open/close хук).

---

### Task 1: Парсеры USB descriptor'ов

**Files:**
- Create: `kernel/src/drivers/bus/usb/dev/desc.rs`
- Create: `kernel/src/drivers/bus/usb/dev/desc/tests.rs`
- Modify: `kernel/src/drivers/bus/usb/mod.rs` (`pub mod dev;` в корне
  модуля добавить позже в таске 3 вместе с реестром; здесь — только
  `pub(super)`-видимость внутри `dev`)

**Interfaces:**
- Produces:

```rust
pub const DESC_DEVICE: u8 = 0x01;
pub const DESC_CONFIGURATION: u8 = 0x02;
pub const DESC_INTERFACE: u8 = 0x04;
pub const DESC_ENDPOINT: u8 = 0x05;
pub const CLASS_HID: u8 = 0x03;

pub struct DeviceDesc { pub max_packet_size0: u16, pub vendor: u16,
    pub product: u16, pub n_configs: u8 }
pub struct EndpointDesc { pub address: u8, pub attributes: u8,
    pub max_packet: u16, pub interval: u8 }
pub struct IfaceDesc { pub class: u8, pub protocol: u8,
    pub endpoints: [EndpointDesc; 4], pub n_endpoints: u8 }
pub struct ConfigDesc { pub interfaces: [IfaceDesc; 2], pub n_ifaces: u8 }
pub enum DescError { Short, BadType(u8), BadLen(u8) }

pub fn parse_device(b: &[u8]) -> Result<DeviceDesc, DescError>
pub fn parse_config(b: &[u8]) -> Result<ConfigDesc, DescError>
```

- [ ] **Step 1: тесты-фикстуры.** В `desc/tests.rs`: корректный
  18-байтный device descriptor (bLength=18, тип 1, max_packet_size0=64,
  vid=0x044f, pid=0xb10a); корректный config blob: 9-байный header +
  9-байтный interface (class 0x03, 1 endpoint) + 7-байтный endpoint
  (address 0x81, attributes 3 = interrupt, max_packet 8, interval 10).
  Негативные: буфер < 18; bLength=0; bDescriptorType≠ожидаемого;
  total_len меньше header'а; config с 5 endpoint'ами у одного
  интерфейса → `BadLen`.
- [ ] **Step 2:** `cargo test -p onyx_kernel` → падают: `parse_device`
  не существует.
- [ ] **Step 3:** Реализовать `desc.rs`: разбор руками по смещениям
  (как joydev-модуль komnata), без `unsafe`, `checked_sub` на всех
  смещениях. `parse_config` идёт по цепочке bLength'ов, INTERFACE
  добавляет endpoint'ы в bounded-массив, переполнение → `BadLen`.
- [ ] **Step 4:** `cargo test -p onyx_kernel` — PASS; `cargo fmt`.
- [ ] **Step 5:** Commit: `feat: usb descriptor parsers for device and config blobs`

### Task 2: Setup-пакеты + тестируемая шина

**Files:**
- Create: `kernel/src/drivers/bus/usb/dev/requests.rs`
- Create: `kernel/src/drivers/bus/usb/dev/requests/tests.rs`

**Interfaces:**
- Consumes: `usb::control_transfer` (существующий).
- Produces:

```rust
pub const REQ_SET_ADDRESS: u8 = 0x05;
pub const REQ_GET_DESCRIPTOR: u8 = 0x06;
pub const REQ_SET_CONFIGURATION: u8 = 0x09;

pub fn setup(rtype: u8, request: u8, value: u16, index: u16, len: u16) -> [u8; 8]
pub fn get_descriptor(typ: u8, index: u8, len: u16) -> [u8; 8]
pub fn set_address(addr: u8) -> [u8; 8]
pub fn set_configuration(cfg: u8) -> [u8; 8]

/// Тестируемая абстракция над control-путём одного контроллера.
pub trait ControlBus {
    fn control(&mut self, setup: &[u8; 8], data: Option<&mut [u8]>,
        data_in: bool) -> KResult<u32>;
}
/// Живая шина: оборачивает usb::control_transfer; caller держит USB-лок.
pub struct LiveBus;
impl ControlBus for LiveBus { /* unsafe fn new() + forwarding */ }
```

- [ ] **Step 1:** Тесты: байтовая точность —
  `get_descriptor(DESC_DEVICE, 0, 64)` == `[0x80,0x06,0x01,0x00,0x00,0x00,0x40,0x00]`;
  `set_address(5)` == `[0x00,0x05,0x05,0x00,0x00,0x00,0x00,0x00]`;
  `set_configuration(1)` == `[0x00,0x09,0x01,0x00,0x00,0x00,0x00,0x00]`.
- [ ] **Step 2:** Прогон → FAIL (нет функций).
- [ ] **Step 3:** Реализовать `requests.rs` (LE-сборка 8 байт).
- [ ] **Step 4:** Прогон → PASS; fmt.
- [ ] **Step 5:** Commit: `feat: usb standard request builders and ControlBus trait`

### Task 3: Enumeration + реестр устройств

**Files:**
- Create: `kernel/src/drivers/bus/usb/dev/mod.rs` (реестр+lock)
- Create: `kernel/src/drivers/bus/usb/dev/enum_seq.rs` (последовательность)
- Create: `kernel/src/drivers/bus/usb/dev/tests.rs`
- Modify: `kernel/src/drivers/bus/usb/mod.rs` (`pub mod dev;`)
- Modify: `kernel/src/srv/main/early/probe.rs` (вызов после init контроллера)

**Interfaces:**
- Consumes: Task 1 (`parse_device`, `parse_config`, `CLASS_HID`), Task 2
  (`ControlBus`, `get_descriptor`, `set_address`, `set_configuration`).
- Produces:

```rust
#[derive(Clone, Copy, PartialEq)]
pub enum Owner { Xhci, Ohci, Ehci }

pub const MAX_SLOTS: usize = 4;
pub struct UsbSlot { pub active: bool, pub owner: Owner, pub addr: u8,
    pub vendor: u16, pub product: u16, pub class: u8, pub iface: u8,
    pub ep_in: u8, pub max_packet: u16, pub interval: u8 }

pub fn enumerate(bus: &mut dyn ControlBus, owner: Owner) -> Result<UsbSlot, Errno>
pub fn slot(idx: usize) -> Option<UsbSlot>          // под USB_DEV_LOCK
pub fn set_slot(idx: usize, s: UsbSlot) -> KResult<()>
pub fn first_stick_slot() -> Option<usize>          // class == CLASS_HID
static USB_DEV_LOCK: SpinLock (приватный)
```

`enumerate` по шагам: control-чтение 8 байт dev-desc на addr 0 →
`max_packet_size0`; `set_address(1)`; повторный GET_DESCRIPTOR device
(18 байт); GET_DESCRIPTOR config (9 байт → total_len; затем полный
blob, буфер 256 байт, обрезать по total_len); `parse_config`; выбрать
первый interface `class == CLASS_HID` с interrupt-IN endpoint'ом
(attributes&3 == 3, address&0x80 != 0); `set_configuration(1)`; вернуть
заполненный `UsbSlot`. Любая ошибка посередине → `Err`, слот не
занимается (заносится только в `set_slot` вызывающим после успеха).

- [ ] **Step 1:** Тест с `FakeBus` (скриптованный `ControlBus` в
  tests.rs: sequence of (setup bytes → canned response)): happy path по
  фикстурам таска 1 → `UsbSlot` с ep_in=0x81, max_packet=8,
  class=0x03, owner передан как есть. Ошибочный скрипт: шина отвечает
  `Err(Errno::Io)` на 3-м запросе → `enumerate` = `Err`, и второй
  успешный `enumerate` занимает первый слот (реестр не забит мусором).
  Битый descriptor (truncated ответ шины) → `Err`.
- [ ] **Step 2:** Прогон → FAIL.
- [ ] **Step 3:** Реализовать `dev/mod.rs` (реестр-массив, как
  `INPUTS` в virtio_input) и `enum_seq.rs`. `LiveBus` не трогает
  реестр; `probe.rs`: в обеих ветках после успешного init контроллера
  вызвать `dev::enumerate(&mut LiveBus, owner)` + `dev::set_slot` (за
  USB_DEV_LOCK), при успехе `kinf!("usb", "dev vid=%x pid=%x class=%x")`,
  при ошибке — `kwrn!` без падения boot.
- [ ] **Step 4:** `cargo test -p onyx_kernel` PASS; `cargo kbuild`
  собирается; fmt/Clippy.
- [ ] **Step 5:** Commit: `feat: usb enumeration core with device slot registry`
  (в коммит-месседже сослаться на закрытие TODO(2026-11-09) в hcd.rs и
  удалить сам TODO-комментарий).

### Task 4: xHCI interrupt-IN

**Files:**
- Create: `kernel/src/drivers/bus/usb/xhci/xfer/intr.rs`
- Create: `kernel/src/drivers/bus/usb/xhci/xfer/intr/tests.rs`
- Modify: `kernel/src/drivers/bus/usb/xhci/xfer/mod.rs` (export)
- Modify: `kernel/src/drivers/bus/usb/mod.rs` (`interrupt_transfer`)
- Modify: `kernel/src/drivers/bus/usb/dev/mod.rs` (arm-вызов)

**Interfaces:**
- Consumes: `dev::slot(idx)` (ep_in, max_packet, interval), существующие
  `ring::enqueue_trb`, `ring::ring_doorbell`, `wait_transfer_event`
  (`xhci/xfer/mod.rs`), контекст слота из `xhci/init.rs`.
- Produces:

```rust
// xhci/xfer/intr.rs
/// DCI = 2*ep_num + 1 для IN (EP1 IN → DCI 3); TRB type Configure Endpoint = 12.
pub(crate) unsafe fn xhci_arm_interrupt(ep: u8, max_pkt: u16, interval: u8) -> KResult<()>
pub(crate) unsafe fn xhci_poll_interrupt(buf: &mut [u8]) -> KResult<u32>

// usb/mod.rs — диспетчер по владельцу слота
/// # Safety: слот idx должен быть active и armed; caller держит USB_DEV_LOCK.
pub unsafe fn interrupt_transfer(slot_idx: usize, buf: &mut [u8]) -> KResult<u32>
```

- [ ] **Step 1:** Host-тесты TRB-строителей `intr.rs` (байтовая точность
  Interrupt-TRB: TRB_TYPE=3 Interrupt, IOC-бит, tdpc/-frame-id по
  образцу существующих builders в `xfer/tests.rs`) — строители
  чистые функции, MMIO в тестах не трогается.
- [ ] **Step 2:** Прогон → FAIL.
- [ ] **Step 3:** Реализовать: input-context + Configure Endpoint для
  DCI interrupt-IN, ring в `G_XHCI.xfer_rings[dci]`, N буферов по
  max_pkt (8–64 байта) предпостятся как в virtio_input
  (`N_EVENTS`-паттерн), `xhci_poll_interrupt` читает used-кольцо,
  ре-постит буфер, возвращает длину.
- [ ] **Step 4:** Прогон → PASS; `cargo kbuild`; fmt/Clippy; каждый
  unsafe — SAFETY-комментарий.
- [ ] **Step 5:** Commit: `feat: xhci interrupt-IN transfers for HID polling`

### Task 5: OHCI periodic + двойной init (SG2000)

**Files:**
- Create: `kernel/src/drivers/bus/usb/ohci/intr.rs`
- Create: `kernel/src/drivers/bus/usb/ohci/intr/tests.rs`
- Modify: `kernel/src/drivers/bus/usb/ohci/mod.rs` (PLE, HCCA-инициализация)
- Modify: `kernel/src/drivers/bus/usb/mod.rs` (`init_usb`: оба контроллера)

**Interfaces:**
- Consumes: ED/TD-строители и DMA-пулы `ohci/control.rs`
  (`MAX_OHCI_ED` и т.д.), реестр `dev::UsbSlot` с `Owner::Ohci`.
- Produces:

```rust
// ohci/intr.rs — чистые (host-тестируемые) строители + MMIO-постановка
pub(super) fn intr_ed_fields(addr: u8, mps: u16, interval_ms: u8) -> u32
pub(super) fn intr_td_fields(max_pkt: u16) -> u32
/// # Safety: OHCI инициализирован; HCCA выделен и валиден.
pub unsafe fn ohci_arm_interrupt(slot_idx: usize) -> KResult<()>
pub unsafe fn ohci_poll_interrupt(buf: &mut [u8]) -> KResult<u32>
```

- [ ] **Step 1:** Тесты строителей: `intr_ed_fields` — адрес в
  FunctionAddress-поле, MPS в разряде 16, формат-поле нулевое;
  `intr_td_fields` — DataPID IN, CC=NOT_ACCESSED, буфер ≥1 байта.
- [ ] **Step 2:** Прогон → FAIL.
- [ ] **Step 3:** Реализовать: HCCA (256 байт, 32 interrupt-указателя)
  из PMM, `OHCI_HC_HCCA` (0x18) запись, `OHCI_HC_PERIODIC_START`
  (0x40), PLE-бит (`1 << 2`) в HcControl, ED/TD в periodic-список,
  poll по TD-комpletion (CC != NOT_ACCESSED).
- [ ] **Step 4:** `init_usb()`: на SG2000 инициализировать EHCI **и**
  OHCI (сейчас — только первый найденный); перечисление FS-устройства
  идёт через OHCI-порты (владелец слота — `Owner::Ohci`), HS — через
  EHCI. EHCI-ветка читает `PORT_OWNER` при отказе перечисления на
  порту и оставляет порт OHCI. QEMU/железо не проверяется здесь —
  только host-тесты строителей и сборка.
- [ ] **Step 5:** Прогон PASS; `cargo kbuild`; fmt/Clippy.
- [ ] **Step 6:** Commit: `feat: ohci periodic interrupt transfers and dual-controller init`
  (в месседж: валидация на живом Duo S отложена до появления платы).

### Task 6: HID report descriptor parser

**Files:**
- Create: `kernel/src/drivers/bus/usb/hid/mod.rs` (корень модуля)
- Create: `kernel/src/drivers/bus/usb/hid/report.rs`
- Create: `kernel/src/drivers/bus/usb/hid/report/tests.rs`
- Modify: `kernel/src/drivers/bus/usb/mod.rs` (`pub mod hid;`)

**Interfaces:**
- Produces:

```rust
pub const HID_DESC_REPORT: u8 = 0x22;   // GET_DESCRIPTOR type
pub const MAX_FIELDS: usize = 64;

#[derive(Clone, Copy, PartialEq)]
pub enum FieldKind { Axis(u8), Button(u8) }  // id — по порядку появления
pub struct ReportField { pub kind: FieldKind, pub bits: u8, pub offset_bits: u16 }
pub struct HidTable { pub fields: [ReportField; MAX_FIELDS], pub n_fields: usize,
    pub report_len_bits: u16 }
pub enum HidError { Short, UnknownItem(u8), TooManyFields, NotStick }

pub fn parse_report_desc(desc: &[u8]) -> Result<HidTable, HidError>
pub fn decode_report(t: &HidTable, report: &[u8],
    out: &mut [(FieldKind, i16); MAX_FIELDS]) -> usize
/// js-критерий: есть хоть одна Axis (Generic Desktop X/Y/Z/Rx/Ry/Rz/Slider/Hat).
pub fn has_axes(t: &HidTable) -> bool
```

Hat switch (usage 0x39) → **две оси** (dx/dy со значениями -1/0/1,
null-state 0/0), как ABS_HAT у joydev. Ось id присваивается по
порядку появления. Report ID не поддерживается (T.16000M их не
использует; если первый байт не item — `Short`/`UnknownItem`).

- [ ] **Step 1:** Тесты. Фикстура A — синтетический descriptor в духе
  T.16000M: GenericDesktop-page, Usage X/Y/Z/Rz (16 бит, logical
  -32767..32767), Slider, Hat (4 бита, 0..7), Button-page 16 кнопок
  (по 1 биту). Фикстура B — минимальная tablet: X/Y absolute 16 бит +
  2 кнопки. Ожидания: количество полей, bits/offsets, id осей по
  порядку, hat → 2 оси. `decode_report`: байты `[0x00, 0x80, 0xFF, ...]`
  → Axis(0) = -32768-толерантно (LE i16), hat=5 → hat-x=0, hat-y=1.
  Негативные: буфер короче report_len → `Short`; descriptor с 70
  полями → `TooManyFields`; неизвестный item-tag → `UnknownItem`.
- [ ] **Step 2:** Прогон → FAIL.
- [ ] **Step 3:** Реализовать парсер: главные items
  (Usage Page 0x05, Usage 0x09, Logical Min 0x15/Max 0x25, Report Size
  0x75, Report Count 0x95, Input 0x81, Output 0x91, Collection
  0xA1/End 0xC0), длинные item'ы (2-байтовый префикс) — все
  смещения через `checked_*`.
- [ ] **Step 4:** Прогон → PASS; fmt/Clippy.
- [ ] **Step 5:** Commit: `feat: hid report descriptor parser with joystick fixtures`

### Task 7: HID-сессия: attach + poll

**Files:**
- Create: `kernel/src/drivers/bus/usb/hid/session.rs`
- Create: `kernel/src/drivers/bus/usb/hid/tests.rs`
- Modify: `kernel/src/srv/main/early/probe.rs` (attach после enumerate)

**Interfaces:**
- Consumes: Task 2 `ControlBus`, Task 3 `dev::slot/first_stick_slot`,
  Task 4/5 `usb::interrupt_transfer`, Task 6
  `parse_report_desc/decode_report/has_axes`.
- Produces:

```rust
#[derive(Clone, Copy, PartialEq)]
pub enum HidEvent { Axis { axis: u8, value: i16 },
                    Button { index: u8, down: bool } }

/// GET_DESCRIPTOR(HID report) → parse → arm interrupt → baseline.
/// Не-stick (has_axes == false) → Err(HidError::NotStick), слот не HID-сессия.
pub fn attach(slot_idx: usize, bus: &mut dyn ControlBus) -> KResult<()>
/// Один poll-тик: interrupt-чтение armed-слота → decode → diff с
/// baseline → события. Baseline-момент выдаётся с init-флагом.
pub fn poll() -> Option<(HidEvent, bool /*is_init*/)>
```

`poll()` вызывается из `input::event::poll_all()` (таск 8) и из
devfs-пути. Diff с baseline: событие генерируется только на изменение
(оси — при отличии от baseline, кнопки — по фронту). Init-события
выдаются один раз при первом успешном репорте (как дренаж
`JS_EVENT_INIT` в komnata session).

- [ ] **Step 1:** Тест: скриптованная сессия — `FakeBus` отдаёт
  report-descriptor из фикстуры таска 6, затем через тестовый хук
  подаётся репорт (чистый `decode+diff` без MMIO: тестируемая часть —
  функция `apply_report(&mut session, bytes) -> [(HidEvent, bool)]`,
  она же используется прод-путём). Ожидания: первый репорт → все оси
  как init; повтор того же репорта → пусто; сдвиг оси → одно
  событие; нажатие кнопки → фронт.
- [ ] **Step 2:** Прогон → FAIL.
- [ ] **Step 3:** Реализовать `session.rs` (таблица-хранение на слот,
  baseline `[i16; 16]` + кнопки `u32`). В `probe.rs`: после
  успешного `enumerate` вызвать `hid::attach`; не-stick → `kinf!` и
  оставить слот без HID-сессии (usb-kbd из smoke — именно этот путь).
- [ ] **Step 4:** Прогон → PASS; `cargo kbuild`; fmt/Clippy.
- [ ] **Step 5:** Commit: `feat: hid session attach and diff-based event stream`

### Task 8: Unified Event + маппер курсора + claim

**Files:**
- Modify: `kernel/src/drivers/input/event/mod.rs` (варианты + drain)
- Create: `kernel/src/drivers/input/event/stick_cursor.rs`
- Create: `kernel/src/drivers/input/event/stick_cursor/tests.rs`
- Modify: `kernel/src/drivers/input/event/mod.rs` тест-мод при необходимости

**Interfaces:**
- Consumes: Task 7 `hid::poll`.
- Produces:

```rust
// event/mod.rs — новые варианты (курсорная семантика ниже):
Event::Stick { axis: u8, value: i16 }
Event::StickButton { index: u8, down: bool }

// stick_cursor.rs
pub const DEAD_ZONE: i16 = 3000;
pub const GAIN_SHIFT: u32 = 12;          // dx = value >> 12 → max ±7 px/тик
pub const CURSOR_AXIS_X: u8 = 0; pub const CURSOR_AXIS_Y: u8 = 1;
pub const CURSOR_BUTTON: u8 = 0;        // кнопка 0 = левый клик
pub fn set_claimed(claimed: bool)       // вызов из devfs open/close (таск 9)
pub fn claimed() -> bool
pub fn map(event: HidEvent) -> Option<Event>  // чистая, для тестов
```

Правило: пока `!claimed()`, `hid::poll` события идут и в кольцо
/dev/js (таск 9), и `map()` → `dispatch()` (Stick с осями 0/1 →
`Event::MouseRel`, кнопка 0 → `Event::MouseButton{Left}`). После
claim — только кольцо. Dead zone: |value| < DEAD_ZONE → нет
движения.

- [ ] **Step 1:** Тесты `map()`: `Axis { axis: 0, value: 2000 }` → None
  (dead zone); `Axis { axis: 0, value: 20000 }` →
  `MouseRel { dx: 4, dy: 0 }`; `Axis { axis: 1, value: -20000 }` →
  `MouseRel { dx: 0, dy: -4 }`; `Button { index: 0, down: true }` →
  Left down; `Button { index: 0, down: false }` → Left up;
  `Button { index: 3, down: true }` → None. Интеграционно через
  `dispatch` + чтение mouse-состояния (как в существующих
  `mouse/tests.rs`): `set_claimed(true)` → Stick-события не двигают
  курсор.
- [ ] **Step 2:** Прогон → FAIL.
- [ ] **Step 3:** Реализовать; `poll_all()` получает второй drain:
  за `DRAIN_LOCK` дополнительно `hid::poll` → кольцо js + map/dispatch
  (порядок: кольцо первым, чтобы claim не терял события).
- [ ] **Step 4:** Прогон → PASS; fmt/Clippy.
- [ ] **Step 5:** Commit: `feat: stick events in unified input model with cursor mapper`

### Task 9: devfs /dev/jsN + open/close-хук

**Files:**
- Create: `kernel/src/fs/devfs/js.rs`
- Create: `kernel/src/fs/devfs/js/tests.rs`
- Modify: `kernel/src/fs/devfs/mod.rs` (lookup/readdir/inode)
- Modify: `kernel/src/fs/devfs/vnode.rs` (read/stat)
- Modify: `kernel/src/fs/vfs/fd/file.rs` (open-хук)

**Interfaces:**
- Consumes: Task 7 `HidEvent`, Task 8 `stick_cursor::set_claimed`,
  источники ms-uptime (тот же, что в `srv/wallclock`).
- Produces:

```rust
pub const DEVFS_JS_BASE_INO: u32 = 80;   // js_ino(i) = 80 + i, i < 4
/// joydev-совместимая запись: u32 time_ms LE, i16 value LE, u8 type, u8 number.
pub const JS_EVENT_BUTTON: u8 = 0x01;
pub const JS_EVENT_AXIS: u8 = 0x02;
pub const JS_EVENT_INIT: u8 = 0x80;
pub fn js_ino(idx: u32) -> u32
pub fn is_js_ino(ino: u32) -> Option<usize>
/// Пусто и нет init-бёрста → Errno::Again. Есть данные → до len/8 записей.
pub unsafe fn js_read(idx: usize, buf: *mut u8, len: u32) -> KResult<u32>
/// Публичный пуш для poll-пути (таск 8): событие → запись в кольцо слота.
pub fn push_event(slot_idx: usize, ev: HidEvent, init: bool)
```

Кольцо на слот: 64 записи × 8 байт, static, SpinLock; переполнение —
терять старые (как cancel-политика PointerQueue — не выдумывать
клики). При `open` (хук в `file.rs` по образцу `DEVFS_PTMX_INO`-ветки)
→ `set_claimed(true)` + в кольцо синтезируется init-бёрст из baseline
сессии; при `close` (по образцу PTY-master free-пути) →
`set_claimed(false)`.

- [ ] **Step 1:** Тесты: `is_js_ino(80)` → Some(0), `is_js_ino(83)` →
  Some(3), `is_js_ino(84)` → None; `push_event` 3 события →
  `js_read` отдал 3 записи с правильными байтами (проверить LE
  побайтово) и типом (axis/button/init-бит); пустое кольцо →
  `Err(Errno::Again)`; init-бёрст: push с init=true читается первым.
- [ ] **Step 2:** Прогон → FAIL.
- [ ] **Step 3:** Реализовать `js.rs` + врезки: `devfs::lookup`
  (`jsN` парсится как `fb`), `readdir_entry`, `vnode::read` ветка
  до `_ => Err(Errno::NoSys)`, `stat` (0o100600), open/close хуки.
  Перед выбором базы убедиться, что 80..83 не пересекается с
  фактическими диапазонами fb (`DEVFS_EXTRA_FB_BASE_INO` растёт с
  числом дисплеев) и PTY-мастерами (`ptym_ino`); при коллизии —
  сдвинуть базу и константу вместе (тест `is_js_ino` едет вместе с ней).
- [ ] **Step 4:** Прогон → PASS; `cargo kbuild`; fmt/Clippy.
- [ ] **Step 5:** Commit: `feat: /dev/jsN joydev-compatible stick nodes with claim hooks`

### Task 10: jstest (OnyxApps)

**Files:**
- Create: `OnyxApps/apps/jstest/main.c` (+ `Makefile` по образцу соседей)
- Test: `OnyxApps/tests/native/` (декод js_event в C)

**Interfaces:**
- Consumes: `/dev/js0` (Task 9): open → init-бёрст, затем события;
  пусто → read вернёт код `Errno::Again` (-19) — retry-цикл с
  `nanosleep(10ms)`.

- [ ] **Step 1:** Host-тест `tests/native`: байты
  `{00 00 e8 03, 34 12, 02, 03}` декодируются в time=1000,
  value=0x1234, axis 3; кнопка: type 0x01; init-бит 0x80 распознаётся.
- [ ] **Step 2:** Прогон (`make -C tests/native test`) → FAIL (нет binary).
- [ ] **Step 3:** Реализовать `main.c` (≤200 строк): open, цикл
  read(8), при EAGAIN — nanosleep и повтор, печать
  `axis 3 = 4660 (init)` / `button 7 down`. Ошибки — в stderr, exit 3
  (по духу fabrilis-joystick). Регистрация в CI-сборку apps.
- [ ] **Step 4:** `make -C tests/native test` PASS; сборка app
  (`.onx` с магией ONX1, как у соседей).
- [ ] **Step 5:** Commit в **OnyxApps**: `feat: jstest app for /dev/js0 diagnostics`

### Task 11: QEMU smoke + синхронизация планов

**Files:**
- Create: `OnyxOS/scripts/qemu-usb-smoke.sh`
- Modify: `OnyxKernel/todo.md` (строка в P2-архитектуру + закрытие
  TODO(2026-11-09) уже учтено в таске 3)
- Modify: `OnyxOS/docs/dev/roadmap.md` (v0.5/v0.6 пункт USB)

**Interfaces:**
- Consumes: готовые ядро (smode) + `qemu-xhci` из
  `qemu-xhci-smoke.sh` (smode-билд, `-machine virt`, таймаут-паттерн).

- [ ] **Step 1:** Скрипт по образцу `qemu-xhci-smoke.sh`: два прогона —
  (а) `-device qemu-xhci -device usb-kbd` → grep лога на
  `usb: dev vid=0627` (перечисление; НЕ ждём js-узел — kbd не stick);
  (б) `-device usb-tablet` → grep `usb: dev` + `js0` (stick-сессия).
  По одному устройству за прогон (решение из design-дока).
- [ ] **Step 2:** Прогон (а) → ожидаемо: если usb-kbd не
  перечисляется — отладить enumeration по логам, пофиксить в
  соответствующем таске, перезапустить.
- [ ] **Step 3:** Прогон (б): tablet должен дать js-узел; вручную
  проверить в interactive-режиме (`run-qemu.sh dev`): `ls /dev` показывает
  `js0`, `jstest` печатает init-оси, движение мыши-tablet меняет оси.
- [ ] **Step 4:** `qemu-smoke.sh` + `qemu-interactive-smoke.sh` — не
  сломаны (регрессия базовой загрузки).
- [ ] **Step 5:** Синхронизация: `todo.md` (закрыть строку USB в P2,
  добавить P2 «multi-slot xHCI/multi-device»), `roadmap.md`,
  README-формулировка про USB (см. «Документация / долг» в todo).
- [ ] **Step 6:** Commits: OnyxOS `build: qemu usb hid smoke script`;
  OnyxKernel `docs: sync usb hid status in todo and roadmap`.

### Финальный гейт (после всех тасков)

- [ ] `cargo test -p onyx_core -p onyx_kernel` — все PASS.
- [ ] `bash scripts/quality/check.sh` (из OnyxOS) — fmt/Clippy без
  dead-code bypass по всей матрице.
- [ ] Обе ветки smoke из таска 11 PASS; отчёт: что прошло, что
  отложено (Duo S — железо, multi-device — backlog).
