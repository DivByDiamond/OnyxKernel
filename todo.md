# OnyxKernel — TODO

> **Актуально на 2026-10-25.**
> Правила кода: `../info.md` (корень набора репозиториев), `docs/CONCURRENCY.md`
> (локи, планировщик, инварианты), `docs/TESTING.md` (что и как проверять).
> История расследований и закрытые баги — `docs/history/` и `git log`.
> Как вести этот файл — последний раздел.

## 1. P0 — блокеры

| ID | Проблема | Детали | Критерий закрытия |
|----|----------|--------|-------------------|
| OC2R-1 | Guest fault после save/reopen мира при расширенном RGB | Реальный OC2R (Fabric 26.1.2 и 26.2): `/bin/fb-draw` + проверка пикселей — базовые R/G/B проходят, а расширенный RGB → reload дважды выявил guest trap на обеих версиях; контрольный повтор прошёл, причина не установлена. Отчёт: `build/reports/onyx-live-acceptance/report.md`. См. `docs/TESTING.md` §11. | Воспроизведено и исправлено, либо доказано, что reload ни при чём |
| OC2R-2 | OnyxFS snapshot полного системного root | 197 блоков до добавления probe против лимита snapshot в 31; `snapshot_create` → `-12`/`NoMem` на обеих версиях. Минимальный root проходит (create=1, list=1, rollback=0, байты восстановлены). | Root-snapshot работает, либо лимит осознанно поднят и задокументирован |

## 2. P1 — ближайшие задачи

| ID | Задача | Где | Статус / что осталось |
|----|--------|-----|------------------------|
| F1 | **FAT32: запись через VFS** | `fs/fat32/write/**`, `fs/vfs/fd/rw.rs`, `node/{create,unlink}.rs`, `meta/truncate.rs` | Драйвер записи написан (`write`/`create`/`unlink`/`truncate_chain`), но VFS отдаёт `NoSys` для `Fs::Fat32` — код мёртвый. Подключить write/create/unlink/truncate + синхронизация размера в dirent |
| F2 | **FAT32: LFN** | `fs/fat32/helpers.rs`, `dir/`, `write/` | Чтение: `scan_dir_entries` пропускает LFN, `readdir_entry` отдаёт только 8.3. Запись: `create` пишет только 8.3. Нужны разбор LFN-цепочек (N×13 UTF-16, checksum, 0x40-флаг) и генерация LFN при создании |
| N1 | **TCP: recv-буфер и EOF** | `net/tcp/{conn,sock,state}.rs` | В рабочем дереве: recv-кольцо 2 KiB → 8 KiB (`RECV_BUF_SIZE`, окно = свободное место), EOF по FIN уже был (`Ok(0)`, тесты `net/tcp/recv_tests.rs`). Осталось: живой QEMU-тест больших ответов |
| N2 | **obrowse: стадии C/D вживую** | `OnyxApps/apps/obrowse` | Стадии A–D в коде; нужен сквозной прогон URL → DNS → TCP → GET → HTML → render → переход по ссылке → back |
| D1 | **UART IRQ-driven RX** | `drivers/uart.rs`, PLIC | Сейчас только LSR-poll в `getc`, ни одной регистрации в PLIC. Нужен RX-буфер + обработчик PLIC |
| D2 | **SBI: spec version + SRST** | `arch/sbi.rs`, `arch/asm/mtrap*.rs`, `syscall/power_sys.rs` | В `sbi.rs` только legacy v0.1 `set_timer`. Нужны базовое расширение (`get_spec_version`, `get_impl_id`, `probe_extension`) и SRST (`system_reset`), причём и через собственный M-mode шим (`-bios none`), и через OpenSBI (`smode`) |
| S1 | **`$5$` sha256crypt** | `core/src/crypto/kdf.rs` + новый модуль | Текущая схема несовместима с glibc: hex вместо crypt-base64, фиксированные 10k раундов вместо A/B/DP/DS. Нужны: совместимый алгоритм, rounds, миграция старых хэшей (старые обязаны продолжать проверяться), locked-политика сохраняется |
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
| L1 | `readdir`/`lookup` видят только 8.3-имена (LFN не парсится) | `fs/fat32` |
| L2 | FAT32 write-пути не вызываются из VFS (`NoSys`) | `fs/vfs/fd/rw.rs` |
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
- ⚠️ Snapshot полного системного root: `NoMem` (см. OC2R-2).

## 6. Принятые компромиссы (не баги)

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
