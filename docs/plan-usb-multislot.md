# Plan: USB multi-slot / multi-device в xHCI

Статус: реализовано 2026-11-22 (этап 1 из 3: multi-slot; этап 2 — live-проверка
`jstest` по `/dev/jsN`). Родитель: `docs/plan-joystick-usb-hid.md`.

## Что было

Boot-проба `bringup_device()` брала первый подключённый порт и клала всё
единственное устройство в глобалы `G_XHCI` (`slot`, `dev_port`, `dev_speed`,
`xfer_rings[32]`, `G_INTR`, `PARKED`). HID-слой (`hid/session`, `/dev/jsN`)
уже сессионный и индексируется по слоту реестра `usb::dev`, поэтому узкое
место было только в xHCI: одно устройство, одно кольцо EP0, одно armed-прерывание,
события общего event ring разбирались только по endpoint id.

## Решение

1. **Состояние по устройству** (`xhci/devs.rs`): `XhciDev` на каждый
   controller slot id (`DEVS[slot_id-1]`, до 8): порт, скорость, свои кольца
   (`rings[DCI-1]`, null `base` = не опубликовано), реестр страниц, канал
   прерывания с собственной FIFO отложенных завершений.
2. **Роутинг событий**: общий event ring разбирается по (slot id, DCI),
   а не только по DCI (`event_completes(ev, slot, dci)`). Завершение чужого
   armed-эндпоинта, встреченное при ожидании/опросе другого устройства или
   внутри `submit_command`, паркуется у владельца (`route_park`), не теряется.
3. **Хэндл устройства в реестре**: `UsbSlot.hc_slot` (xHCI slot id; 0 для
   EHCI/OHCI), `ControlBus::hc_slot()`, `LiveBus::with_hc_slot`. xHCI
   control/interrupt/bulk адресуются по slot id; USB-адрес не участвует.
4. **Перечисление всех портов**: `bringup_device(port) -> slot_id`; проба
   (`usb_probe::probe_xhci_ports`) проходит все подключённые порты, пока в
   реестре есть место. Реестровый индекс = номер `/dev/jsN`.
5. **Очистка ошибок**: каждая страница устройства (контекст, кольца, буферы)
   записывается в `PageSet`; input-контексты живут на scratch-странице
   (`with_scratch_page`, освобождается на всех путях). Сбой bring-up/arm
   откатывается до чекпоинта; устройство, отвергнутое реестром (hub, не-HID,
   реестр полон), получает Disable Slot + освобождение страниц
   (`release_device`). Если контроллер не подтвердил Disable Slot, страницы
   намеренно НЕ освобождаются (DMA мог остаться) — kwrn.
6. Команды Address/Configure/Enable теперь проверяют Completion Code;
   `submit_command` подтверждает ERDP и паркует transfer-события; HID `poll`
   обходит слоты round-robin (`rr`), чтобы один стик не голодал соседей.

## Не входит

EHCI/OHCI multi-device (на SG2000 по одному устройству на контроллер),
хаб-перечисление и hot-plug (Port Status Change), cleanup ошибок
`xhci::init` (контроллер целиком не поднялся), runtime-teardown сессии.

## Проверка

- host: `cargo test -p onyx_core -p onyx_kernel` (тесты: `xhci/devs/tests.rs`,
  `xhci/pages/tests.rs`, `xfer/tests.rs`, `device/tests.rs`, `dev/requests/tests.rs`);
- `OnyxOS/scripts/qemu-usb-smoke.sh`: прогоны (c) два usb-tablet →
  js0+js1, (d) usb-kbd + usb-tablet, (e) usb-hub + usb-tablet (release слота);
- `qemu-smoke.sh`, `qemu-interactive-smoke.sh`, `qemu-xhci-smoke.sh`.

## Этап 2 (2026-11-22): live-валидация jstest

`OnyxOS/scripts/qemu-jstest-smoke.sh` (+ `qemu-jstest-drive.py`): S-mode ядро,
диск OnyxFS, qemu-xhci + usb-tablet, QMP `input-send-event`. Проверено live:
init-бёрст js0, live оси и кнопка 0 down/up, чистый выход `jstest -t`. Claim курсора
теперь per-slot. Не подтверждено live: события второго стика (см. todo.md, USB).
