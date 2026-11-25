# OnyxKernel — TODO

> **Актуально на 2026-11-15.**
> Правила кода: `../info.md` (корень набора репозиториев), `docs/CONCURRENCY.md`
> (локи, планировщик, инварианты), `docs/TESTING.md` (что и как проверять).
> Обязательный полный fmt/Clippy и запрет dead-code bypass: `docs/QUALITY.md`.
> История расследований и закрытые баги — `docs/history/` и `git log`.
> Как вести этот файл — последний раздел.

## 1. P0 — блокеры

Активных P0 нет. Проверенные OC2R-1/2 перенесены в §5.

## 2. P1 — ближайшие задачи

_Сделано 2026-10-25 и ушло в архив: TCP recv-окно 8 KiB (`e0dc146`), FAT32
write/create/unlink/truncate через VFS + LFN + FSINFO (`5e92115`), совместимый
с crypt(3) `$5$` sha256crypt (`5b21d00`), UART RX через PLIC с кольцом 4 KiB
(`drivers/uart_rx.rs`), SBI base + SRST на обеих цепочках (`arch/sbi.rs`,
`arch/asm/mtrap*.rs`) с `reboot`/`poweroff` в шелле, obrowse вживую через
QEMU slirp (`N2`: рендер → переход по ссылке → back) и PTY-мультиплексор
`mux` (`U1`). По пути найдены и исправлены три дефекта: `TcpConn` копировался
по значению и пробивал 64-КиБ kstack, `onyxcc` обрезал смещения > 2047 до
12 бит (`addi`), и `onyxcc` знаково расширял 32-битные беззнаковые константы
(`TIOCGPTN` уходил в ядро как `0xFFFFFFFF80045430`). Лимит памяти сборки
6 GiB — `OnyxOS/scripts/mem-limit.sh`._

| ID | Задача | Где | Статус / что осталось |
|----|--------|-----|------------------------|
| F8 | [x] Обнулять вновь открытый хвост блока при shrink → extend | `fs/onyxfs/write/meta/extend.rs` | 2026-11-15, `9215876`: регрессии direct/indirect, внутри блока и через границы; 70 core + 3 integration + 291 kernel, 4 init прошли |
| EX1 | [x] Отклонять недопустимые ONX permissions до маппинга, проверять PA перед копированием | `proc/onx/load.rs`, `segments.rs` | 2026-11-15, `14937bc`; согласовано с linker `e7a57ec`: 293 kernel-теста, QEMU linked-C++ и отказ malformed image без падения shell |
| FS-J1 | [x] Fault injection на первом rollover журнала при большом truncate | `fs/onyxfs/journal.rs`, `write/meta/truncate.rs` | 2026-11-15, `cf6ca42`: full-block undo, dedup и откат по внешней операции; write-cut регрессии и QEMU legacy/undo прошли (`docs/ONYXFS_JOURNAL.md`, границы модели там же) |
| FS-J2 | [x] Безопасное повторное использование журнала после ошибки очистки | `fs/onyxfs/journal.rs` | 2026-11-15, `cf6ca42`: header-only commit, quarantine, restartable recovery; ошибка clear → короткая транзакция → recovery покрыта регрессией |
| MK1 | [x] `mkimage`: вложенный каталог создавался повторно (`/usr/share/music` дважды, утечка inode и блока) | `tools/src/mkimage/tree.rs` | 2026-11-21: поиск существующего каталога через `find_parent_dir`; тест `tree/tests.rs` падает на старом коде; заодно битовая карта больше не помечает занятым лишний последний inode (`next_ino - 1`) |
| FS-R1 | [x] `rename` поверх существующего файла не освобождал вытесненный inode и его блоки (утечка при каждой перезаписи `/etc/passwd`/`shadow` через temp-файл); rename на самого себя — no-op, каталог поверх имени — `EISDIR`/`ENOTDIR`/`EEXIST` | `fs/onyxfs/ops/dir/rename.rs` | 2026-11-21: `free_inode` в той же транзакции; тесты `rename_over_a_file_frees_the_replaced_inode_atomically` (обрыв на каждой записи) и `rename_onto_itself_or_a_directory_keeps_everything` падали на старом коде |

## 3. P2 — backlog по подсистемам

### Сеть
- Блокирующий `recv` (сейчас userspace busy-poll; в ядре нет wait-очереди по сокетам).
- Отправка: 536 байт на сегмент, IP-фрагментации нет — поднять MSS/учесть MTU пути.
- Server sockets (`listen`/`accept`): реализованы для root-only admin-сокетов — проверить и описать контракт в README.
- TLS — вне ядра; obrowse остаётся HTTP-only.

### ФС
- Динамическая загрузка модулей (сейчас `/proc/modules` показывает только встроенные).
- [x] FAT32: per-FS лок, write-пути сериализованы (`b1270dc`, 2026-11-09).
- [x] xHCI: URB transfer, диспетчеризация и PCI-обнаружение (`9d83db4`, `f99101e`, `7790fdf`, 2026-11-09). В QEMU с `-device qemu-xhci` контроллер инициализируется: base 0x4000_0000, 64 слота, 8 портов. Smoke: `OnyxOS/scripts/qemu-xhci-smoke.sh` (`8c5ad8f`). Перечисление устройств, HID-разбор и interrupt-IN готовы и проверены LIVE в QEMU 2026-11-20 (ветка `a50d5fb`..`c3c619a` + live-фиксы `fe80f57`, `1e2d1f5`, `b7885a1`, `a181b22`): smoke `OnyxOS/scripts/qemu-usb-smoke.sh` — usb-kbd перечисляется без js-узла, usb-tablet даёт stick-сессию `/dev/jsN`.

### USB (HID / джойстик)
- [x] Multi-slot xHCI / multi-device. Закрыто 2026-11-22: состояние по устройству (`xhci/devs.rs`), роутинг общего event ring по (slot id, DCI) с парковкой чужих завершений, `UsbSlot.hc_slot`, проба перечисляет все подключённые порты, несколько одновременных `/dev/jsN` (индекс = слот реестра), очистка PMM/слотов на ошибочных путях (`PageSet`, Disable Slot для отвергнутых устройств), round-robin опрос HID. План: `docs/plan-usb-multislot.md`. Smoke `qemu-usb-smoke.sh` (c)-(e): два usb-tablet → js0+js1, usb-kbd+usb-tablet, usb-hub+usb-tablet. Не охвачено: EHCI/OHCI multi-device, хаб-перечисление/hot-plug, cleanup ошибок `xhci::init`, live-проверка потока событий двух стиков (этап 2, jstest).
- [x] Claim курсора per-slot (2026-11-22, этап 2): `stick_cursor::G_CLAIMED` заменён счётчиками `G_CLAIMS[MAX_SLOTS]` (`claim`/`release`/`claimed(slot)`); close js-fd снимает claim только своего узла, два читателя одного узла считаются; `feed_stick` гейтит маппинг курсора по слоту источника (`forward_stick`). Тесты: `stick_cursor/claim_tests.rs`. Остаток: close без latched-open (нет сессии) или двойной close dup-fd декрементирует (насыщение на 0) — нет per-fd учёта.
- [x] Исправлено при live-прогоне jstest (2026-11-22): readdir `/dev` показывал `js00` (`devfs::copy_name` не писал NUL, хвост прошлого имени `blk0`); тест `readdir_names_are_nul_terminated_over_a_dirty_buffer`.
- [ ] Live-поток второго стика: в QEMU (2 usb-tablet) события второго tablet не доходят до `/dev/js1` (трасса: его wakeup кикает xHCI slot 1; не выяснено, особенность QEMU xhci или адресация слотов ядра). js0 проверен live (`qemu-jstest-smoke.sh`). Нужна диагностика на реальном железе/другим хост-инъектором.
- [x] Первый принятый HID-отчёт помечался init. Закрыто 2026-11-25: при attach baseline берётся через GET_REPORT (`hid/session/fetch.rs::fetch_session`); STALL/короткий ответ — прежнее поведение. Live в QEMU (usb-tablet): до фикса первое движение шло как `(init)`, после — живым событием. На реальном железе не проверено.
- [x] Фоновый запуск `cmd &` в osh падал page fault (`sepc=0x17e16 stval=1`). Закрыто 2026-11-25: `fork_child_frame` подменял потомку `a1/a2` на argv/envp, а syscall ABI меняет только `a0` — osh держал в `a1` длину среза `args` через `ecall`. Теперь потомок получает все регистры родителя; поля `Proc.argv_sp/envp_sp` удалены. Тест `child_frame_preserves_every_register_except_the_return_value`.
- [ ] (2026-11-25) `fork` не копирует адресное пространство: родитель и потомок делят всю память, включая стек. Фон и пайплайны osh сейчас работают, но это зависит от раскладки стека, выбранной компилятором. Нужна копия user-страниц (или COW).
- [ ] EHCI interrupt-передачи: HS-HID сейчас честный `NoSys` + kwrn (решение 1 плана джойстика); нужен periodic-список по образцу OHCI (`ohci/intr.rs`).
- [ ] Live-валидация Duo S (SG2000): чек-лист — arm/poll frame-timing и head-writeback в OHCI, PLE-планирование, обход HCCA-списка, семантика CBP-writeback, kwrn на отказ init каждого контроллера, повторный аудит утечек PMM/пула на ошибочных путях arm (сейчас всё проверено только в QEMU/xHCI-пути).

### Платформа / время
- [x] RTC → `gettimeofday`: epoch-база из RTC, fallback на uptime (`1d94693`, 2026-11-09).
- rv32 non-smode: `nanosleep` через прямой CLINT MMIO вместо SBI (нужен `mtrap_32`-путь, как у rv64).

### Userland: GUI (v0.7+)
- [x] D1: onyxdesk v1 — десктоп-демон (C99, ring 1): claim `/dev/fb0` на сеанс, панель + лаунчер из `/etc/desk.conf`, managed-запуск приложений с возвратом по выходу | `OnyxDesktop` + `OnyxOS/scripts` (упаковка `/bin/onyxdesk` + `/etc/desk.conf`, smoke `qemu-desk-smoke.sh`) | готово: 237 host-проверок, smoke-маркеры `DESK_READY`/`APP_STARTED osnake`/`APP_RETURN`/`DESK_EXIT 0` — 2026-11-21, `OnyxDesktop 7e131ec`
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

### Структура кода
- Сверка 2026-11-15: inline `mod tests { ... }`, `pipeline_tests` и `modifier_tests` в `kernel/core/init/tools/src` больше не найдены; прежний счётчик 53 устарел. Раскладка новых тестов — по §8.7.
- [x] Q1: полный fmt/Clippy без dead-code bypass (2026-11-15, `1c0f188`): общие init-модули и host-тесты очищены, `scripts/quality/check.sh` и CI проходят всю поддерживаемую матрицу; RV32 smode явно отклоняется вместо ELF с entry=0.

### Документация / долг
- [x] README: «16 FD slots» сверить с кодом; «UDP/DHCP/DNS — осталось» устарело (реализовано); синхронизировать `README.ru.md`. USB-формулировка синхронизирована 2026-11-20: перечисление/HID/interrupt — готово (live-smoke), осталось multi-device/EHCI-interrupt/mass-storage. Закрыто 2026-11-21, `2e5f475`: «16 FD slots» подтверждено кодом (`PROC_MAX_FDS: usize = 16`, `kernel/src/proc/process/types.rs:30`, тест `proc/tests.rs:149`); UDP/DHCP/DNS отражены как реализованные; RU вкатан в единый `README.md`, `README.ru.md` удалён.
- [x] `OnyxOS/docs/dev/roadmap.md` отстал от кода (v0.4/v0.5-чекбоксы). Закрыто 2026-11-21, OnyxOS `67ac3ed`: срез 2026-11-21 записан в roadmap (сверка по heads, смоки boot/interactive/desk PASS, CI зелёный), статус архивного линкера дополнен по `OnyxCompiller/docs/STATUS.md`, `roadmap.en.md` синхронизирован (OnyxDesktop-секция, USB HID, срез).

## 4. Открытые дефекты и ограничения

| ID | Симптом / ограничение | Где |
|----|------------------------|-----|
| L3 | Нет честного EOF-сигнала после RST (`Err(Io)` вместо `Ok(0)`) — сознательно, приложение отличает сброс от FIN | `net/tcp/sock.rs` |
| L4 | Glyph-blit по MMIO попиксельный (scroll/clear — word-копии) | `drivers/video/fb_term` |
| L5 | Tearing/двойная буферизация на живом мониторе OC2R не проверены | `drivers/video/fb` |
| L6 | Journal «железный» цикл на OC2R (запись → kill VM → reboot → данные целы) не прогнан | `fs/onyxfs` |
| L7 | ЗАКРЫТО 2026-11-14: файлы длиннее 10 прямых блоков растут, усекаются и удаляются (три причины, см. архив ниже; регрессии в `fs/onyxfs/large_file_tests.rs`) | `fs/onyxfs/write` |

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
- ✅ OC2R-1 закрыт в OC2R `58a3e8be` ([PR #60](https://github.com/DivByDiamond/OC2R/pull/60), CI ещё идёт):
  shutdown ждёт worker и немедленно помечает чанк для финального CPU save до
  Fabric unload. На исходном ядре 66148d… + Sedna 5.2 проходят по три
  последовательных reload на Fabric 26.1.2 и 26.2; CPU-регистры/CSR и четыре
  RAM-модуля побайтно совпали frozen → final save → restore. Новые ответы
  shell, RAM-переменная, private-root файл и RGB565 проходят во всех 6 циклах.
  Это исправление чистого shutdown, не гарантия атомарности при crash/autosave
  или обычной выгрузке чанка. Отчёт OC2R: `build/reports/onyx-live-acceptance/trap-diagnosis/postfix-final/README.md`.
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
- 2026-11-14: L7 (файлы >40 KiB не росли выше 45056 байт и не усекались) закрыт. Подозрение на `G_BUF` в
  `free_indirect_block` подтвердилось, но это была лишь одна из трёх причин. (1) `write` держал косвенный блок в
  общем `G_BUF`, а `alloc_data_block` читает в него битмап: после выделения первой записи косвенный блок
  перезаписывался битмапом, следующая запись брала мусорный номер блока и падала с `Io`. (2) `free_indirect_block`,
  `free_double_indirect` и `shrink` читали записи из того же `G_BUF`, который затирает `free_data_block`, так что
  освобождались случайные блоки или усечение падало с `Inval`; `O_TRUNC` игнорирует ошибку, отсюда «старое
  содержимое». (3) Журнал на 32 записи переполнялся на любой операции с >31 блоком (усечение файла от ~120 KiB):
  `journal_log` теперь закрывает накопленную часть транзакции и продолжает. Отдельно: `create` по существующему
  имени подменял dirent новым inode и терял старый вместе с блоками (`cp` поверх большого файла выедал том), теперь
  существующий файл усекается и переиспользуется, каталог даёт `EISDIR`. Формат на диске не менялся. У `onyxfs` теперь
  есть хост-стенд (`TEST_DISK` в `cfg(test)`), проверено ещё и в QEMU-госте.
- 2026-11-13: удалённая оболочка OC2R (`rshd`, живёт в репозитории мода) упёрлась в три дыры ядра, закрыты тремя
  коммитами: `spawn` наследует привязанные fd 0..2 (`51963a4`, раньше `cmd > file` не касался внешних программ),
  `O_APPEND` пишет в конец файла при каждой записи (`49bc246`), `mkdir` существующего имени отвечает `EEXIST`
  вместо подмены каталога пустым (`302e484`; `init` каждый boot делал `mkdir /etc/init` и стирал маркеры
  включённых сервисов). Проверено в QEMU-госте, юнит-тесты на копирование fd и на выбор смещения записи.
  `mkdir`-ветка юнит-тестом не покрыта: у `onyxfs` нет хост-стенда.
- 2026-11-10: SHA-256 — блочный `update`, развёрнутая компрессия и
  `onyx_core` на `-O3` (логин: KDF −45 %, `$5$` −47 %, 1 MiB −49 %); цена —
  +16 KB `.text` в четырёх init-бинарниках, ядро −2 KB. Замеры, отброшенные
  варианты — `docs/history/2026-11-sha256-throughput.md`.
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
7. **Тесты — только в отдельных файлах.** В файле с кодом допустима лишь
   строка `#[cfg(test)] mod tests;`. Сам модуль живёт в `tests.rs` рядом
   (для `mod.rs`) или в подпапке `foo/tests.rs` (для `foo.rs`). Inline
   `mod tests { ... }` в файле с кодом запрещён. Причина: файл остаётся
   ≤250 строк, диффы кода и тестов не смешиваются, а подпапка не раздувает
   счётчик «≤4 файла на папку». `#[cfg(test)]`-хелперы, нужные только
   тестам, переносятся вместе с ними.
