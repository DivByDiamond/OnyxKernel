# OnyxKernel — TODO

> **Актуально на 2026-10-25.**
> Правила кода: `../info.md` (корень набора репозиториев), `docs/CONCURRENCY.md`
> (локи, планировщик, инварианты), `docs/TESTING.md` (что и как проверять).
> История расследований и закрытые баги — `docs/history/` и `git log`.
> Как вести этот файл — последний раздел.

## 1. P0 — блокеры

| ID | Проблема | Детали | Критерий закрытия |
|----|----------|--------|-------------------|
| OC2R-1 | Guest fault после save/reopen мира при расширенном RGB | Трассировка OC2R подтвердила worker после последнего CPU save при живой mmap RAM. В Fabric unload очищает отложенный dirty-флаг до финальной записи; исправление shutdown/final-save проверяется в OC2R. Это не подтверждённый дефект RGB или ядра. Отчёт: `build/reports/onyx-live-acceptance/trap-diagnosis/`. | Повторные reload с точным сравнением финальной CPU/RAM-пары проходят на обеих версиях |

## 2. P1 — ближайшие задачи

_Сделано 2026-10-25 и ушло в архив: TCP recv-окно 8 KiB (`e0dc146`), FAT32
write/create/unlink/truncate через VFS + LFN + FSINFO (`5e92115`), совместимый
с crypt(3) `$5$` sha256crypt (`5b21d00`), UART RX через PLIC с кольцом 4 KiB
(`drivers/uart_rx.rs`), SBI base + SRST на обеих цепочках (`arch/sbi.rs`,
`arch/asm/mtrap*.rs`) с `reboot`/`poweroff` в шелле. Лимит памяти сборки
6 GiB — `OnyxOS/scripts/mem-limit.sh`._

| ID | Задача | Где | Статус / что осталось |
|----|--------|-----|------------------------|
| N2 | **obrowse: стадии C/D вживую** | `OnyxApps/apps/obrowse` | Стадии A–D в коде; нужен сквозной прогон URL → DNS → TCP → GET → HTML → render → переход по ссылке → back |
| U1 | **PTY-мультиплексор (userspace)** | `fs/pty/*` готово; приложение — нет | Ядро PTY готово (2026-09-01: 4 пары, `/dev/ptmx`, `/dev/pts/N`, winsize, poll, O_NONBLOCK). Не написан сам мультиплексор (tmux-подобный: панели, переключение, resize) |

## 3. P2 — backlog по подсистемам

### Сеть
- Блокирующий `recv` (сейчас userspace busy-poll; в ядре нет wait-очереди по сокетам).
- Отправка: 536 байт на сегмент, IP-фрагментации нет — поднять MSS/учесть MTU пути.
- Server sockets (`listen`/`accept`): реализованы для root-only admin-сокетов — проверить и описать контракт в README.
- TLS — вне ядра; obrowse остаётся HTTP-only.

### ФС
- Динамическая загрузка модулей (сейчас `/proc/modules` показывает только встроенные).
- FAT32: нет per-FS лока; все write-пути предполагают single-hart I/O.
- xHCI: URB transfer (EHCI control/bulk уже есть).

### Платформа / время
- RTC → `gettimeofday`: драйвер (`drivers/platform/rtc.rs`) есть, но время берётся из uptime.
- rv32 non-smode: `nanosleep` через прямой CLINT MMIO вместо SBI (нужен `mtrap_32`-путь, как у rv64).

### Userland: GUI (v0.7+)
- Window manager (создание/удаление окон).
- Compositor (z-order, clipping).
- Mouse cursor + click handling.
- Продвинутые виджеты (ScrollView, Menu, Dialog).

### Userland: Java runtime
- Class loader (`.class`: constant pool, поля, методы).
- Интерпретатор байткода (стек, локалы, базовые инструкции).
- Подмножество JDK (`java/lang/Object`, `String`, `System`, `arraycopy`).
- GC (mark-sweep), исключения, потоки поверх proc/scheduler.
- hello-world от javac → `/bin/jvm` в QEMU/OC2R.

### Документация / долг
- README: «16 FD slots» сверить с кодом; «UDP/DHCP/DNS — осталось» устарело (реализовано); USB-формулировка (URB-слой есть, открыт только xHCI transfer); синхронизировать `README.ru.md`.
- `OnyxOS/docs/dev/roadmap.md` отстал от кода (v0.4/v0.5-чекбоксы) — обновить при следующем релизном срезе.

## 4. Открытые дефекты и ограничения

| ID | Симптом / ограничение | Где |
|----|------------------------|-----|
| L3 | Нет честного EOF-сигнала после RST (`Err(Io)` вместо `Ok(0)`) — сознательно, приложение отличает сброс от FIN | `net/tcp/sock.rs` |
| L4 | Glyph-blit по MMIO попиксельный (scroll/clear — word-копии) | `drivers/video/fb_term` |
| L5 | Tearing/двойная буферизация на живом мониторе OC2R не проверены | `drivers/video/fb` |
| L6 | Journal «железный» цикл на OC2R (запись → kill VM → reboot → данные целы) не прогнан | `fs/onyxfs` |

## 5. Проверено на реальном железе (OC2R, 2026-10-25)

Стенд: настоящий `FLASH_MEMORY_ONYXOS`, Fabric 26.1.2 и 26.2, 64 MiB RAM,
встроенный private VirtIO root без съёмного HDD; host-side подстановки
изображений нет. Отчёт, скриншоты и архив стенда:
`build/reports/onyx-live-acceptance/report.md` (для JVM/OC2R — также
`/storage/project/jvm/oc2r/docs/roadmap/onyx-live-acceptance.md`),
чек-лист — `docs/TESTING.md` §11–12.

- ✅ Загрузка `smode`-ядра (`9bec335`) и вход в root shell.
- ✅ Сеть: DHCP вместо хардкода IP (стек `net/dhcp`, DNS из lease).
- ✅ Framebuffer r5g6b5 на живом мониторе: `/bin/fb-draw`, красный/зелёный/синий
  прошли на обеих версиях Fabric (монитор 320×200×16, проектор 640×480×16).
- ✅ OnyxFS snapshot/rollback на минимальном root (create/list/rollback,
  байты файла восстановлены после изменения).
- ⚠️ Расширенный RGB → save/reopen мира: guest fault (см. OC2R-1).
- ✅ OC2R-2 закрыт (`54c4f34`): полный root содержит 965 data + 12 metadata
  блоков (старый подсчёт 197 пропускал indirect allocations). Packed DEFLATE/raw
  snapshot проходит create/list/mutate/rollback и rollback после реального
  Minecraft world reload на Fabric 26.1.2 и 26.2. Все 7 фаз проходят на каждой
  версии; OC2R private root без съёмного диска. Kernel SHA256:
  `b55d8a1740ce7a72ea1e9e7a559333a980f98da891e4145a4a4ed0640c3702a3`.
  Проверки: 58 core + 3 tools + 234 kernel tests, QEMU boot/interactive.
  Формат и recovery: [ONYXFS_SNAPSHOTS.md](docs/ONYXFS_SNAPSHOTS.md).

## 6. Принятые компромиссы (не баги)

- Packed snapshots читают legacy raw/RLE, но downgrade на старое ядро с записью
  snapshot не поддерживается. В существующей области 1 MiB помещается одна полная
  копия этого root; следующая может вернуть NoSpace.
- `lto = false`: fat/thin LTO ломают линк ядра (`__rust_alloc` после LTO-merge).
- KDF без memory-hardness — осознанная позиция (10k SHA-256 до перехода на `$5$`).
- Точечные clippy-исключения — обоснованы в коде; новых `#[allow(dead_code)]` быть не должно.
- Остатки правила «≤4 файла на папку» (`fdt`, `onyxfs`) — осознанно приняты.
- `onyx_init`-бины не собираются под host-тесты (сырой RISC-V asm).
- Псевдо-ФС (`/proc`, `/dev`, `/ipc`) не поддерживают `create`/`unlink` — так и задумано.

## 7. Архив

- `docs/history/2026-09-investigations.md` — дословный архив старого `todo.md`:
  чёрный экран (ramfb/таймер/шрифт), SMP respawn (4 волны), rv32-первый запуск
  (5 багов), сеть/ohttp (errno, virtio-net TX/legacy, порты UDP/TCP), аудиты
  2026-08-24/27, SAFETY-волны, dead-code чистки и т.п.
- `docs/history/YYYY-MM-<тема>.md` — формат для новых расследований.
- 2026-10-25: TCP recv-окно 8 KiB (`e0dc146`) и FAT32 write/LFN (`5e92115`),
  проверено в QEMU + mtools + `fsck.fat` на сыром FAT32-диске.
- Закрытые пункты (>30 дней) удаляются из этого файла в архив.

## 8. Как вести этот файл

1. **Todo — это список работ, а не отчёт.** Разборы инцидентов, таблицы замеров
   и логи — в `docs/history/` или `docs/TESTING.md`, здесь только ссылка.
2. **Одна задача = одна строка/строка таблицы**: ID, что сделать, где, критерий
   «готово». Никаких простыней на 40 строк внутри чекбокса.
3. Статусы: `[ ]` открыто, `[~]` частично (обязательно указать, что осталось),
   `[x]` готово → при следующей ревизии уезжает в архив/`git log`.
4. Каждое изменение статуса — с датой; закрытие — с хешем коммита.
5. P0 — только то, что ломает данные/железо/релиз. Всё «хотелось бы» — P2.
6. Если задача расползлась на несколько подсистем — заводи отдельные ID
   (например `F1`, `F2`) и ссылайся между ними.
