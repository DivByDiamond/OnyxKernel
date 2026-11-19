# Plan: USB HID-джойстик (T.16000M) — ввод в Onyx

Статус: draft (2026-11-19). Обсуждался выбор подходов A/B/C, выбран **A**
(USB HID-стек целиком, без virtio-input-ступени).

## Intent и зачем

Хозяин OS хочет поставить Onyx на RISC-V-одноплатник, воткнуть в USB
джойстик Thrustmaster T.16000M (HID) и управлять курсором и специальными
программами. Хостовой ОС между джойстиком и Onyx нет — ядро Onyx само
должно перечислить USB-устройство и читать его HID-отчёты.

Целевая плата пока не выбрана, но код уже указывает на неё: EHCI/OHCI
проинициированы hardcoded-базами SG2000 (Milk-V Duo S, см.
`drivers/bus/usb/mod.rs::init_usb` и `srv/main/early/probe.rs`). Дизайн
толерантен к плате: всё выше host-controller не зависит от неё.

## Цели

1. USB-устройство, воткнутое в порт, перечисляется ядром: descriptor →
   SET_ADDRESS → configuration → class HID распознан.
2. HID-отчёты (interrupt IN) декодируются в оси/кнопки; T.16000M —
   16 осей и 16+ кнопок.
3. События доступны userspace: узел `/dev/js0`, формат записи — 8 байт
   `js_event` (time_ms: u32 LE, value: i16 LE, type: u8, number: u8) —
   побайтно совместим с Linux joydev, чтобы логика в духе
   `fabrilis-joystick` переносилась без перекодировки.
4. Стик двигает курсор: мёртвая зона + коэффициент; кнопка = клик.
5. Приложение `jstest` (OnyxApps, C99) печатает сырые события —
   калибровка/диагностика, аналог `fabrilis-joystick --list`.
6. Всё проверяемо в QEMU без железа: `qemu-xhci` + виртуальные HID
   (`usb-kbd`, `usb-tablet`), ручной QA — passthrough настоящего стика.

## Non-goals

- Force feedback (у T.16000M его нет).
- Маппинг-профили (RON/JSON) в духе komnata — вне ядра; при желании
  позже как userspace-инструмент над `/dev/js0`.
- USB mass-storage и прочие классы (enumeration их увидит, но драйверов
  не будет).
- Хабы с transaction translators; multi-config/device с не-HID
  интерфейсами внутри composite-устройства — берём первый HID-интерфейс.
- Hot-plug в runtime: enumeration на boot; повторный скан — отдельная
  задача (в roadmap «динамические модули» того же класса).

## Ограничения (из info.md / CONCURRENCY.md)

- ≤250 строк на файл; тесты в `foo/tests.rs`, не inline.
- Никаких `unwrap`/`expect`/`panic!` на путях недоверенного ввода —
  descriptor'ы и HID-репорты приходят «с провода».
- Все USB/HID-коды — именованные константы; lock-дисциплина: вся
  мутация USB-состояния под SpinLock с SIE=0 (как `INPUT_LOCK` в
  `virtio_input`), poll-модель без IRQ (как сейчас весь input).
- devfs: новая серия inode в `fs/devfs`, не пересекающаяся с
  существующими (root 1, fb0 2, blk base 16, hvc0 49, PTY).

## Approaches (рассмотренные)

- **A. USB HID-стек сразу** — выбран: единственный путь, работающий на
  железе; потребляет существующий URB/HCD-слой.
- B. Virtio-input сначала — быстро в QEMU, но на плате virtio-input нет;
  ступень выброшена хозяином.
- C. Гибрид B→A — отклонён тем же решением: единый план без
  промежуточного контракта.

## Дизайн

### Фаза 1 — USB enumeration core

`drivers/bus/usb/dev/` (новый модуль): читает device descriptor по
адресу 0 (первые 8 байт, узнать max_packet_size0), `SET_ADDRESS`,
полные descriptor'ы (device/config/interface/endpoint),
`SET_CONFIGURATION`, выбирает первый HID-интерфейс (class 0x03,
протокол 0), сохраняет адрес/endpoint-таблицу. Реестр устройств —
статический массив слотов, как `INPUTS` в `virtio_input` (lock
`USB_DEV_LOCK`); **слот хранит владеющий контроллер** (Xhci/Ohci/Ehci),
и все передачи идут через владельца слота, а не через глобальный
`G_ACTIVE`. Потребитель закрывает TODO(2026-11-09) в `hcd.rs`.

Использует существующий `usb::control_transfer`.

**Объём по устройствам (решение по builder-check):** цель хозяина —
один стик в одном порту, поэтому первая итерация — **одно устройство**
(текущий xHCI-слот единственный, `wait_transfer_event` остаётся как
есть). Multi-slot xHCI (per-slot rings + demux по slot-id) и общее
мульти-устройство — отдельная строка backlog (P2), не часть этого
плана; smoke подключает по одному HID за раз. Реестр всё равно
массив: расширение слотов позже не меняет структуру.

### Фаза 2 — interrupt IN transfers

- xHCI: TRB-кольцо interrupt-передач, poll used-кольца как в
  `virtio_input` (по образцу существующих transfer-путей).
- OHCI: periodic-список (ED/TD) — на Duo S full-speed-устройство
  уходит в OHCI-компаньон EHCI, так что OHCI-путь обязателен для железа.
- EHCI: interrupt-передачи отложены (HS-джойстики редки; если стик
  окажется high-speed — добавить по мере надобности).
- Контракт: `usb::interrupt_transfer(dev, ep, buf) -> KResult<u32>`,
  poll-модель, один общий lock поверх очереди устройств.

**Роутинг Duo S (решение по builder-check):** сейчас `init_usb()`
инициализирует ровно один контроллер, а `G_ACTIVE` — единственный
enum; при `G_ACTIVE == Ehci` OHCI-компаньон недостижим и
`PORT_OWNER`-handoff в EHCI отсутствует. Решение: на SG2000
инициализировать **оба** контроллера, а владельца устройства
записывать в слот реестра по port-ownership (EHCI-порт с
`PORT_OWNER` отдаёт full-speed-устройство OHCI). Диспетчер по
`G_ACTIVE` остаётся для legacy-вызовов, новые пути идут через
владельца слота. Валидация на живом Duo S — вне этой задачи, но
структуры данных (владелец в слоте) попадают в неё сразу.

### Фаза 3 — HID-класс

`drivers/bus/usb/hid/` (новый): `GET_DESCRIPTOR(HID report descriptor)`
через control transfer; парсер descriptor'а — таблица
usage → (тип, смещение в репорте). Декод interrupt-репортов →
`(axis_index, i16)` / `(button_index, bool)` через таблицу; всё
без unsafe-парсинга, породы байтов руками (по духу `joydev`-модуля
komnata). Init-состояние: при первом успехе interrupt-чтения снимок
текущих значений становится baseline (аналог `JS_EVENT_INIT`-дренажа).

**Какие HID становятся js-узлами (решение по builder-check):** только
устройства, в чьём report descriptor есть оси Generic Desktop
(X/Y/Z/Rx/Ry/Rz/Slider/Hat). usb-клавиатура из smoke — проверка
enumeration + interrupt-пути, js-узла и маппера она не получает: её
кнопки — Keyboard-usage, не Generic Desktop Button, и в клики не
превращаются. USB-клавиатура как источник `Event::Key` — отдельная
будущая задача, не эта.

### Фаза 4 — unified events + devfs + курсор

- `drivers/input/event`: вариант `Event::Stick { axis: u8, value: i16 }`
  и `Event::StickButton { index: u8, down: bool }`; `poll_all()`
  расширяется drain-ом USB-HID наряду с virtio-input.
- `fs/devfs/js.rs`: `/dev/jsN` (N по слоту реестра); `read` отдаёт
  8-байтные `js_event`-записи из кольца; при открытии узел выдаёт
  init-пакет (текущее состояние осей/кнопок), как joydev.
- Курсор: kernel-маппер `stick → MouseRel` (мёртвая зона, усиление,
  hat → стрелки не нужно), кнопка 0 → `MouseButton{Left}`. Маппер активен,
  пока `/dev/js0` никто не открыл (claim по факту open, как clone-узел
  `/dev/ptmx`); открыл `jstest` — маппер замолчал, события идут
  читателю. Это одна строка договорённости, не magic.

**Семантика read/claim (решение по builder-check):** devfs сегодня
не имеет операции open — claim-хук добавляется в syscall-пути по
образцу существующего `/dev/ptmx`-хука в `fs/vfs/fd/file.rs`.
Когда кольцо пусто, `read` возвращает `EAGAIN` (poll-сисколла в ядре
нет — todo P1 #1 — и блокировку в ядро не тянем); `jstest` делает
`nanosleep(10ms)`-цикл повторных чтений.

### Фаза 5 — jstest + smoke

- `OnyxApps/apps/jstest` (C99, ≤200 строк): цикл `read` из `/dev/js0`,
  печать `axis 3 = -1234` / `button 7 down`; host-тест в
  `tests/native` на декод `js_event`.
- `OnyxOS/scripts/qemu-usb-smoke.sh`: `qemu-xhci` + `usb-kbd`
  (перечисление + interrupt-путь на виртуальном HID), затем `usb-tablet`
  (HID-декод с осями) — **по одному устройству за прогон** (см. фазу 1);
  ручной прогон — `-device usb-host,vendorid=0x044f` с настоящим
  стиком (нужны права на `/dev/bus/usb`).

## Приёмка

- Host: новые юнит-тесты парсера descriptor'ов (в т.ч. битые/truncated
  входы), decode-таблиц, devfs-узла; `cargo test -p onyx_core -p
  onyx_kernel` зелёный; fmt/Clippy строгий.
- QEMU: usb-smoke (kbd, затем tablet — по одному) зелёный;
  interactive-smoke не сломан.
- Ручной QA при наличии стика: passthrough → `jstest` печатает оси,
  курсор двигается стиком в framework-less QEMU `dev`-конфиге.
- Duo S: вне этой задачи до появления платы; перечисление на EHCI/OHCI
  проверяется тем же кодом через QEMU-путь (xHCI) + дизайн оставляет
  OHCI-interrupt обязательным для железа.

## Решения (сводка, по итогам builder-check)

1. **Одно устройство** в первой итерации; multi-slot xHCI — backlog P2.
2. **Владелец-контроллер хранится в слоте реестра**; на SG2000
   инициализируются оба контроллера (EHCI + OHCI-компаньон).
3. **js-узел только для HID с Generic Desktop-осями**; usb-kbd в smoke
   ничего не инжектит и не кликает.
4. **`EAGAIN` на пустом кольце** + retry-цикл в `jstest`; claim —
   open-хук по образцу `/dev/ptmx` в `fs/vfs/fd/file.rs`.

## Открытые вопросы (минимум)

1. Сколько USB-слотов в реестре — стартово 4 (как порты qemu-xhci по
   умолчанию), расширяемо.
2. Скорость poll-цикла для interrupt IN: вписаться в существующий тик
   input-poll (10 мс) — HID-джойстику достаточно.
3. Имена узлов: `/dev/js0` — принято по аналогии с joydev; счётчик
   с нуля по слотам реестра.

## Отложено строителю

- Точная структура periodic-списков OHCI (по образцу существующих
  `ohci/control.rs`, `ehci/queue.rs`).
- Разбивка файлов внутри новых модулей под лимит 250 строк.
- Формат klog-сообщений по ходу фаз.
