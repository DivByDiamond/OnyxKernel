# OnyxKernel — TODO

## 📏 Правила проекта (обязательны для всего кода и всех агентов):

1. **Максимум 250 строк на файл** — файл перерос → дробить по ответственности.
2. **Максимум 4 файла на папку** — одна папка = одна задача/подсистема;
   в идеале один файл = одна ответственность. Перебор → раскидывать по подпапкам.
3. **KISS / DRY / SOLID**: минимальное рабочее решение; дубли — в общие функции
   (в т.ч. между kernel/core/init); SRP — модуль меняется по одной причине.
4. **Раскидка кода по подпапкам → сразу запускать сабагента на аудит всего проекта**
   (структура, границы модулей, зависимости, нарушения правил 1–3).
5. **Аудит-агент обязан удалять все `#[allow(dead_code)]` и прочие заглушочные allow**
   по всему проекту: мёртвый код — удалить, живой, но не подключённый — подключить
   или честно пометить TODO с датой.

## 🎯 DEADLINE: 15 сентября 2026 (см. PLAN.md для детального расписания)

## ✅ Готово (архив, детали в git-истории):

- Rust-рерайт (~98%, global_asm!), динамические процессы, 3 ring'а + syscall ACL, 85 syscall'ов
- OnyxExec v2, OnyxFS v2 (timestamps, indirect blocks, write + journal recovery), RLE onx-загрузка
- Flashback snapshots, IPC channels + /ipc/* (ipcfs), procfs, libfdt, PLIC dispatch
- Userland: login/osh/passwd/useradd/userdel, /etc/passwd+shadow ($5$+миграция), first-boot setup
- SMP: боевой (mailbox в OnyxBoot, per-hart bootstrap, -smp 2), per-CPU runqueues, steal,
  sched_setaffinity; CPU affinity, load balancing
- Framebuffer (32bpp, PSF1/PSF2, Unicode + UTF-8), fb_term, bootargs (loglevel, console=/fb=)
- UART: FIFO + Ctrl+C→SIGINT; virtio-blk/net/rng/gpu/console/input, SDHCI, gmac, xhci/ehci/ohci
- QEMU smoke-инфраструктура (headless + interactive), kdump/panic recovery
- Аудиты 2026-08-24/27: волны 1–3 закрыли все блокеры B1–B9 (см. git f36c989..523a967);
  переполнения → overflow-checks=true; KAT SHA256 (RFC 6234) + kdf-тесты в onyx_core;
  cargo test: onyx_core 121, onyx_kernel 121; CI (fmt/clippy strict, build-матрица rv64+rv32);
  файлы ≤250 строк, allow-зачистка, dead-code удалён (canaan_eth, xhci/mass, core/types.rs…)
- SAFETY-комментарии волна 1 (2026-08-29): drivers/arch/mm — 80 файлов, ~1400 строк,
  0 непокрытых unsafe-блоков в drivers/vmm/trap; smp.rs → smp/ + secondary.rs
- GITHUB: комментарии кода — только английский (проверка scripts/check_no_cyrillic.sh)
- P1 concurrency fixes (2026-08-31): fork race (ProcState::Creating + publish_ready),
  waitpid race (B4 protocol), sched_setaffinity UAF (proc_list_lock), net sync (recursive
  NET_LOCK), procfs per-hart buf, chown policy, libfdt bounds — см. docs/CONCURRENCY.md
- Lua runtime (2026-08-31): VM foundation + stdlib (string/table/math) + REPL demo + syscall
  bindings — userspace программа в ring2, /bin/lua (84KB)
- TUI library stubs (2026-08-31): Widget trait + Button/Label/TextBox заглушки + tui_demo
  (null fb pointer, text rendering TODO) — начальная структура, не рабочая

## ❌ КРИТИЧНО ДО 15 СЕНТЯБРЯ:

### 🔥 Найдено и починено при живом тесте ohttp в QEMU (2026-09-01)
Тестировал `OnyxApps/apps/ohttp` в QEMU (`-netdev user` + `guestfwd`); по ходу
нашлись и почищены два системных бага, плюс остался один открытый:

- [x] **Errno не транслировался в POSIX** — `core/src/errno.rs::Errno::as_i64()`
      отдавал «сырой» внутренний ordinal (`Ok=0, NoMem=-1, Inval=-2, NoEnt=-3,
      Io=-4, Perm=-5, ...`), и именно это число (после отрицания) попадало в
      userspace `errno`, хотя `libonyxc/include/io/errno.h` документирует и
      определяет POSIX/glibc-значения (`ENOENT=2, EIO=5, EACCES=13, ...`) и
      прямо утверждает, что трансляция уже происходит. Добавлены
      `Errno::to_posix()` / `Errno::from_i64()` / `Errno::translate_syscall_result()`
      (`core/src/errno.rs`, покрыты host-тестами) и единая точка перевода в
      `kernel/src/syscall/handler/dispatch.rs::handle()` (оборачивает и ACL-early-return,
      и результат матча по `nr`). Подтверждено живьём: `ohttp` до фикса получал
      `errno=4` (сырой `Io`), после фикса — `errno=5` (POSIX `EIO`).
- [x] **virtio-net не настраивал отдельную TX-очередь (queue 1)** —
      `send()` писал в те же кольца, что `setup_rx_queue` завела для очереди 0
      (RX), и уведомлял `R_QUEUE_NOTIFY` со значением `0` — то есть говорил
      устройству «в RX есть новые буферы» вместо «вот кадр на отправку», и
      затирал RX-дескрипторы 0/1. Заведена отдельная TX-очередь (`queue 1`,
      как у `virtio_console` — см. `kernel/src/drivers/virtio_console/mod.rs`
      для образца), `send()` теперь пишет в `G_NET.tx_*` и уведомляет
      `R_QUEUE_NOTIFY=1` (`kernel/src/drivers/virtio_net/mod.rs`,
      `kernel/src/drivers/virtio_net/xfer.rs`).
- [x] **virtio-mmio регистрировал очереди только по modern-протоколу (v2)** —
      реальный `qemu-system-riscv64 -device virtio-net-device` под `-M virt`
      по умолчанию отдаёт **legacy** транспорт (`R_VERSION=1`), у которого нет
      регистров `R_QUEUE_{DESC,AVAIL,USED}_*`/`R_QUEUE_READY` — вместо них
      нужен один `R_QUEUE_PFN` на физически смежный регион
      desc|avail|used, плюс `R_GUEST_PAGE_SIZE`/`R_QUEUE_ALIGN`. Драйвер писал
      только modern-регистры, так что на legacy-транспорте очередь физически
      никогда не «вооружалась» — отсюда 0 пакетов на pcap. Добавлен общий
      `setup_queue_rings(base, modern)` с legacy-веткой (по образцу
      `kernel/src/drivers/virtio/queue.rs::setup_queue`, который у virtio-blk
      уже делал это правильно). **Подтверждено pcap'ом**: до фикса — 0 пакетов
      за весь бут; после — полный DHCP DISCOVER/REPLY, ARP REQUEST/REPLY,
      TCP SYN → SYN-ACK от пира.
- [x] **DHCP OFFER доходил до RX-буфера корректно, но не засчитывался; TCP
      SYN-ACK матчился с той же ошибкой** — оба `handle_udp_inner`
      (`kernel/src/net/udp/rx.rs`) и `handle_tcp_inner`
      (`kernel/src/net/tcp/handle.rs`) читали src/dst port **в обратном
      порядке**: UDP/TCP-заголовок (RFC 768/793) кладёт source port в байты
      0-1, destination port — в байты 2-3, а оба обработчика были написаны
      наоборот. Для UDP это ломало `sock.local_port == dst_port` (сравнение
      шло с полем source вместо destination — наш сокет на 68 порту никогда
      не матчился с ответом сервера). Для TCP аналогично ломало 4-tuple
      матчинг `c.src_port != dport || c.dst_port != sport` — реальный
      SYN-ACK/ACK никогда не находил живое соединение. **Подтверждено живьём
      end-to-end**: до фикса — DHCP всегда фоллбэк на захардкоженный IP,
      `tcp_connect` — таймаут; после фикса — `[INF] net: DHCP lease
      acquired`, полный TCP handshake+data+FIN с реальным сервером через
      `ohttp` (см. `OnyxApps/todo.md`).
- [x] **`usleep()`/`nanosleep()` зависал навсегда** (2026-09-02, **фикс
      подтверждён живым QEMU-тестом**). Диагностика прошла через 3
      отвергнутых гипотезы (см. git-историю этой строки для деталей) до
      найденного через monitor `info registers` root cause: CLINT
      `mtimecmp` MMIO-компаратор взводит только `mip.MTIP` —
      M-mode-only прерывание, которое RISC-V `mideleg` в принципе не
      может делегировать в S-mode. Кастомный M→S переход ядра при
      загрузке (`arch/asm/boot.rs`) обнулял `mie` и один раз делал
      `mret`, никогда не возвращаясь в M-mode — MTIP→STIP форвардинг не
      происходил вообще, и любой `wfi` в S-mode, ждущий таймер (включая
      `idle.rs` на реально простаивающем харте), не мог проснуться.
      Пробовал современный обход через Sstc (`stimecmp`) — не сработал,
      эта сборка QEMU (`11.1.1`, `-M virt`) не даёт S-mode доступ к
      `stimecmp` даже с `-cpu max`; откачено.
      **Итоговый фикс**: минимальный M-mode trap-хендлер
      (`kernel/src/arch/asm/mtrap.rs`, `mtrap_entry`), реализующий
      классический legacy SBI v0.1 `SBI_SET_TIMER` (уже был частично
      готов — `arch::sbi::set_timer()` раньше использовался только
      `smode`/OC2R-сборкой против настоящего OpenSBI). Обработчик ловит
      два случая: (1) `ecall` из S-mode (бит 9 medeleg намеренно НЕ
      делегирован — раньше был, теперь ловится в M-mode) — пишет CLINT
      `mtimecmp` текущего харта, чистит `mip.STIP`, включает `mie.MTIE`;
      (2) machine timer interrupt (cause 7, не делегируем в принципе) —
      выставляет `mip.STIP` (проброс в S-mode, где `mideleg` уже
      делегирует STI, а `sie.STIE` включён) и гасит `mie.MTIE` (иначе
      re-trap на каждой инструкции, пока компаратор не перевзведён).
      `mscratch` на каждый харт указывает на свою строку
      `G_MTRAP_SCRATCH` (по образцу `sscratch` в `trap_asm.rs`) — ни
      один регистр не трогается без save/restore, MTI может прервать
      произвольный S-mode код. Подключено per-hart: `boot.rs` (hart 0) и
      `arch/smp/secondary.rs` (harts 1..N, у них свой M-mode сетап).
      `srv::timer::arm_timer` унифицирован — `smode` и rv64-non-smode
      теперь оба идут через `arch::sbi::set_timer` (rv32-non-smode пока
      остаётся на старом прямом CLINT MMIO — `mtrap.rs` есть только для
      rv64, см. TODO там же). `sys_nanosleep` возвращён к паттерну
      idle-loop (`timer::init_hart` + `set_sstatus(SIE)` + `wfi` в
      цикле) — теперь реально работает, а не просто выглядит правильно.
      **Живое подтверждение** (QEMU, `-M virt -smp 2`, диагностический
      `/bin/init` = `init/src/sleep_test.rs`, добавлен как
      `[[bin]] onyx-sleep-test`): `before uptime_us=10000` →
      `nanosleep returned 0` → `after uptime_us=510000` — ровно 500мс,
      воспроизведено дважды подряд; полная загрузка боевого образа до
      login prompt тоже проверена (SMP/virtio-blk не сломаны). cargo
      build (rv64+rv32), clippy -D warnings (оба таргета), cargo test
      (146 passed) — всё чисто.
      Заодно найден и исправлен независимый latent-баг, который
      вскрылся при первой Sstc-попытке (когда тики стали доходить до
      S-mode впервые за всю историю этого QEMU-бута):
      `virtio_input::poll()` разыменовывал `G_IN.used` без проверки, что
      устройство вообще было пробировано (`G_IN.base != 0`) — раньше
      никогда не проявлялся именно потому, что таймерные тики не
      доходили. Guard добавлен (`kernel/src/drivers/virtio_input/decode.rs`).
      **Осталось на будущее**: rv32-таргет по-прежнему использует старый
      прямой CLINT MMIO путь для арминга таймера (см. TODO в
      `srv::timer::arm_timer`'s doc comment и `arch/asm/mod.rs`) — то же
      самое MTIP→STIP не-форвардится и для rv32 wfi-ожиданий; не чинил
      (rv32 — вторичный таргет проекта), нужен `arch/asm/mtrap_32.rs` по
      аналогии, если понадобится.
- [x] **Указатели рядом с `USER_TOP` (0x40000000) проваливали
      `user_ptr_ok`/`parse_user_path`, включая `argv`-строки** —
      **СДЕЛАНО 2026-09-03**. `parse_user_path` (`kernel/src/syscall/
      handler/dispatch.rs`) требовала безусловные 256 валидных байт за
      указателем; `argv[N]`, которые лоадер кладёт у самой вершины
      стека (`ustack=0x...3ffffff0` в логах), давали `path + 256 >
      USER_TOP` и получали `EINVAL`, даже когда сама строка целиком лежала
      в замапленной памяти (живой пример: `ohttp` не мог открыть свой же
      `argv[2]`). Исправлено: окно валидации теперь ограничивается
      `min(256, USER_TOP - path)` вместо жёстких 256 — проверяется и
      читается ровно столько байт, сколько реально может уместиться перед
      `USER_TOP`, а не фиксированный запас. `user_ptr_ok` (общая проверка
      диапазона) не трогали — баг был только в этой более строгой
      256-байтовой обвязке для путей/строк.

### 🔥 Приоритет 1 — Non-blocking I/O (БЛОКЕР #1: без него нет htop/UI с таймерами)
**Статус: ✅ СДЕЛАНО 2026-09-01** — poll (#87), FIONREAD, O_NONBLOCK, F_GETFL/F_SETFL, VMIN/VTIME

Любая TUI-программа с автообновлением (htop, osysmon-стиль с `kbhit()`) блокируется
навсегда на `read()` — это единственный принципиальный пробел ABI.

- [x] **`poll()` syscall** (~200 строк):
      - Новый `kernel/src/syscall/poll_sys.rs` + диспетчеризация в `dispatch.rs`
      - Поддержка fd-массива с events/revents (POLLIN/POLLOUT/POLLERR)
      - Интеграция с termios (stdin poll = keyboard input ready)
      - Таймач через uptime_us() для POLLIN+timeout

- [x] **`FIONREAD` — реальный подсчёт** (~30 строк):
      - `kernel/src/syscall/fs_sys3/extra/ioctl.rs:187` — сейчас заглушка (всегда 0)
      - Читать `recv_len` из UDP_SOCKS[fd] или termios input buffer

- [x] **`O_NONBLOCK` — обработка в read/write** (~40 строк):
      - `kernel/src/syscall/fs_sys/read_write.rs` — проверять флаг до блокировки
      - Если nonblock и нет данных → вернуть EAGAIN вместо блокировки

- [x] **`F_GETFL`/`F_SETFL` — реальные флаги** (~20 строк):
      - `kernel/src/syscall/fs_sys/open_close/mod.rs:80` — F_GETFL хардкодит O_RDONLY
      - Хранить flags в fd-таблице, возвращать/обновлять через fcntl

- [x] **`VMIN`/`VTIME` — non-canonical read** (~50 строк):
      - `kernel/src/syscall/fs_sys/read_write.rs:135` — сейчас блокируется навсегда
      - VMIN: прочитать минимум N байт перед возвратом
      - VTIME: таймач между байтами (inter-byte timeout)

### 🔥 Приоритет 2 — Сигналы (БЛОКЕР #2: без него нет job control)
**Статус: ✅ СДЕЛАНО 2026-09-01** — SIGTSTP/SIGCONT (ProcState::Stopped), SIGCHLD + SA_NOCLDWAIT, SIGWINCH, kill для ring 2 (своя группа)

Без SIGCHLD родитель не знает о завершении детей; без SIGTSTP Ctrl+Z убивает процесс
вместо остановки; без SIGWINCH resize не отслеживается.

- [x] **`SIGWINCH` — уведомление при resize** (~40 строк):
      - Генерировать при изменении framebuffer geometry
      - Доставлять foreground-группе (как SIGINT через `signal_foreground`)
      - Добавить в `TIOCGWINSZ` ioctl — уведомлять при первом чтении после resize

- [x] **`SIGCHLD` — авто-доставка родителю** (~60 строк):
      - В `proc/lifecycle/exit.rs`: при exit ребёнкаirim SIGCHLD родителю
      - Реализовать `SA_NOCLDWAIT` (sigaction flags) — auto-reap без zombie
      - Родитель может `waitpid(WNOHANG)` для неблокирующего reaping

- [x] **`SIGTSTP` (Ctrl+Z) — реальный stop** (~50 строк):
      - В `signals/handler.rs`: 상태 Running → Stopped (новый ProcState)
      - Не убивать процесс, а остановить (не ставить в runqueue)
      - `SIGCONT` → Stopped → Ready (возобновление)

- [x] **`SIGCONT` — возобновление** (~30 строк):
      - Посылать при `tcsetattr(TCSANOW, ...)` с resumed状态
      - Если обработчик установлен — вызвать его; иначе — просто de-freeze

- [x] **`kill()` — открыть для ring 2** (~10 строк):
      - `kernel/src/srv/handler/acl.rs:78` — убрать из ring ≤ PROC_RING_ROOT блока
      - Оставить проверку: процесс может слать сигнал только в свою группу

### 🖥️ Приоритет 3 — TUI библиотека (виджеты с реальным рендерингом)
**Статус: ✅ СДЕЛАНО 2026-09-01** — SYS_mouse_read, double buffering, event pump, PSF-текст в виджетах, tui_demo с mmap fb + poll event loop

- [x] **Mouse syscall** `SYS_mouse_read` (#86) (~80 строк):
      - `kernel/src/syscall/input_sys.rs` — virtio-input → (x, y, buttons)
      - Event struct: {x: i16, y: i16, buttons: u8}

- [x] **Double buffering** `fb::swap_buffers()` (~60 строк):
      - `kernel/src/drivers/video/fb/mod.rs` — back buffer (3.7MB) + atomic swap
      - Устраняет tearing при полной перерисовке экрана

- [x] **Event loop** `kernel/src/srv/event.rs` (~100 строк):
      - poll клавиатуры/мыши через virtio-input
      - Таймеры через uptime_us() + callback
      - Интеграция с poll() syscall (P1 #1)

- [x] **Widget text rendering** (~150 строк):
      - `init/src/libtui/widget.rs` — Button/Label рисуют текст через PSF шрифт
      - Использовать существующий `fb::put_char()` из fb_term
      - TextBox: курсор, вставка, удаление

- [x] **tui_demo** — реальный framebuffer (~30 строк):
      - `init/src/tui_demo.rs:24` — заменить `null_mut` на mmap /dev/fb0
      - Добавить event loop (ESC = выход)

### 🔌 Приоритет 4 — OC2R интеграция


- [ ] Проверка загрузки через OnyxOSFirmware блок
      (нужен OC2R-стенд; в песочнице нет qemu/hardware — шаги в PLAN.md)
- [x] Сеть: DHCP вместо хардкода IP (используй существующий UDP стек)
      (уже реализовано и подключено: net/dhcp + srv/main/mod.rs — DHCP
      первый, QEMU user-net только как fallback; G_DNS тоже из lease)
- [~] Framebuffer: тест r5g6b5 режима на OC2R мониторе
      (хост-тест test_rgb565_* покрывает конверсию — НАЙДЕН И ИСПРАВЛЕН
      баг бит-раскладки: поля перекрывались на 16bpp; живой тест монитора
      требует OC2R)
- [ ] Snapshot на несъёмном диске OC2R (нужен OC2R-стенд)

## 📅 ПОСЛЕ 15 СЕНТЯБРЯ (v0.6+):

### Сеть / userland (нужно для obrowse, см. OnyxApps/todo.md)
- [x] **`wfi()`-ожидание глубоко внутри обработчика syscall'а роняло
      ядро в page fault** — **СДЕЛАНО 2026-09-03**. Обнаружено при первой
      попытке использовать SIE+`wfi()`-паттерн `sys_nanosleep` из
      `net::dns_resolve` (вызывается из `sys_net_resolve`, т.е. на два
      уровня глубже, чем сам `sys_nanosleep`, лежащий прямо в теле
      обработчика). Корень бага — не в механике сохранения/восстановления
      trap frame'а (она сама по себе корректна, `sp` честно
      сохраняется/восстанавливается через сам frame), а в
      `srv::trap::handle`: финальная "opportunistic" проверка
      `G_NEED_RESCHED` безусловно звала `sched_yield(tf)` для ЛЮБОГО
      обслуженного прерывания, в т.ч. для прерывания, поймавшего
      процесс посреди ЕГО ЖЕ вложенного kernel-mode кода (наш `wfi()`
      глубоко в `dns_resolve`) — а не только для прерывания,
      заставшего процесс в user-mode (обычный, единственный
      предполагаемый случай верхнеуровневого резюмирования).
      `sched_yield` в таком вызове сохраняет ВЛОЖЕННЫЙ, невернеуровневый
      trap frame как `(*current).tf`; при последующем реальном
      переключении на этот процесс (когда параллельно runnable другой
      процесс, напр. родительский `osh` в `wait()`) резюмирование по
      этому кадру ломает состояние процесса — крашится на его следующем
      syscall'е. Воспроизводилось стабильно и на `-smp 1`.
      Исправлено в `kernel/src/srv/trap.rs`: `sched_yield` в конце
      `handle()` теперь вызывается, только если прерывание либо застало
      idle-харт (`current` = null), либо реально прервало user-mode
      (`sstatus.SPP == 0`, захвачено один раз в начале `handle()` как
      `interrupted_user`) — т.е. только когда `tf` гарантированно
      верхнеуровневый резюмируемый кадр. Для вложенного kernel-mode
      прерывания (SPP=1, `current` не null) тик всё равно полностью
      обслуживается (таймер/watchdog/event pump), просто решение о
      реальном переключении откладывается до следующей безопасной
      точки — `G_NEED_RESCHED` остаётся выставленным и не теряется.
      `net::dns_resolve` вернул `sys_nanosleep`-паттерн SIE+`wfi()`
      вместо busy-poll'а по `read_time()`. Живой QEMU-тест: `dnstest`
      резолвит `example.com` без единого краша на `-smp 1` и `-smp 2`
      (несколько прогонов подряд), `cargo test` — 146/146, `clippy` —
      чисто.
- [x] **DNS-резолвер как syscall** — **СДЕЛАНО 2026-09-03**.
      `net_resolve(name_ptr, ip_out)` (#89, `kernel/src/syscall/
      net_sys.rs`) поверх существующего `net::dns_resolve` (простой
      A-запрос, без кэша/AAAA); libc-обвязка `net_resolve()` в libonyxc
      (`io/net.h`); `ohttp` теперь принимает хостнейм наравне с сырым
      IPv4. Живой QEMU-тест (`scripts/run_qemu_net.sh`, netdev user +
      virtio-net-device): `example.com -> 172.66.147.243`, подтверждено
      pcap-дампом (`-object filter-dump`) и `ohttp`, дошедшим до TCP
      `connect()` на резолвленный адрес.
      По пути найдены и исправлены два независимых бага, из-за которых
      резолвер сначала не работал вообще:
      1. `dns_resolve` слал запрос через `udp_sendto()` (одноразовый
         сокет со своим эфемерным портом, закрывается сразу после
         отправки), а слушал ответ на отдельном сокете от `udp_bind(0)`
         (`local_port=0`) — ответ DNS-сервера адресован порту, с
         которого ушёл запрос, поэтому никогда не совпадал и тихо
         отбрасывался в `handle_udp_inner`. Добавлены `udp_bind_connect`
         + `udp_send_bound` (`kernel/src/net/udp/sock.rs`) — отправка и
         приём теперь идут через один и тот же сокет/порт.
      2. Ожидание ответа считало итерации (`for _ in 0..30000 { poll();
         ... }`) вместо реального времени — на быстром хосте цикл
         пустых MMIO-проверок пролетает за микросекунды, что быстрее
         любого настоящего UDP round-trip. Заменено на дедлайн по
         аппаратному таймеру (`arch::csr::read_time()`, 10 МГц на QEMU
         virt) — 5 реальных секунд ожидания через тот же SIE+`wfi()`
         паттерн, что и `sys_nanosleep` (не busy-poll: планировщик
         теперь безопасно обрабатывает вложенный wfi, см. соседний
         пункт про `sched_yield`/`interrupted_user`).
- [ ] **Отдельный, не связанный с DNS SMP-краш под `-smp 2`** —
      обнаружено 2026-09-03 при живом QEMU-тесте `ohttp` (после
      успешного DNS-резолва и TCP `connect()`, т.е. уже после того как
      главный процесс завершился). Idle-харт (hart 1, `pid=0`) падает с
      "illegal instruction" / чередой "unhandled exception" и
      "KERNEL page fault" с явно битыми полями (отрицательный pid,
      `root_pa=0`) — вывод в консоль сильно перемешан между хартами
      (похоже на гонку за UART при конкурентной печати с двух хартов).
      Тот же класс краша ловился в этой сессии ещё ДО любых правок
      DNS/планировщика (первый прогон `ohttp` под `-smp 2`, ещё
      затрагивавший `pmm::bitmap::bm_get`), т.е. это отдельный,
      pre-existing баг SMP-пути (harts >1), не эффект от
      `net_resolve`/`sched_yield`-фикса. Не чинил — нужен отдельный
      аудит запуска процессов/idle-цикла на вторичных хартах и
      сериализации UART-вывода между хартами.
- [ ] Бо́льшие recv-буферы / recv semantics — `tcp_recv` сейчас никогда
      не возвращает `0` (только `Ok(n>0)` или `Err(NoEnt)` пока нет
      данных, `Err(Inval)` после TIMEWAIT); нет чистого сигнала "peer
      закрыл соединение". `ohttp` обходит это через `Content-Length`
      (см. `OnyxApps/apps/ohttp/README.md`), но `obrowse` для страниц
      без `Content-Length` (chunked/legacy) захочет настоящий EOF.

### PTY + мультиплексоры (~460 строк)
**Статус: ✅ СДЕЛАНО 2026-09-01** — fs/pty (4 пары, 512B-кольца, master-close
→ EPIPE у slave), /dev/ptmx clone-node + /dev/pts/N, ioctl TIOCGPTN /
TIOCGWINSZ / TIOCSWINSZ, poll по реальной заполненности колец,
O_NONBLOCK → EAGAIN; libc: struct winsize + pty_open()

- [x] PTY master/slave pair — fs/pty/ (таблица пар, кольца, side I/O,
      блокирующие stream-хуки через sched_yield, O_NONBLOCK)
- [x] `/dev/ptmx`, `/dev/pts/N` — device nodes (devfs, clone-семантика open)
- [x] Alt-screen `\x1b[?1049h/l` — реальная подмена буфера
      (save/restore whole-surface через pmm-ран + курсор; при нехватке
      памяти — no-op; ansi/render.rs + state.rs)
- [x] `struct winsize` в libc (`libonyxc/include/io/termios.h`)
      + pty_open() helper

Дополнительно в этой же задаче:
- core::ringbuf — общие ring-примитивы (kernel IPC-каналы переехали на них)
- cleanup pre-existing core test lints (const assert, c-строки, unwrap)

### Java runtime (большая цель, ~1-2 месяца работы)
- [ ] 1. Минимальный class loader (.class: constant pool, fields, methods)
- [ ] 2. Интерпретатор байткода JVM (стек, локалы, базовые инструкции)
- [ ] 3. Подмножество JDK (java/lang/Object, String, System, arraycopy…)
- [ ] 4. GC (mark-sweep), исключения, потоки поверх proc/scheduler
- [ ] 5. hello-world javac → /bin/jvm в QEMU/OC2R

**Формат**: JVM-интерпретатор как onx-программа в ring1/2, без изменений ядра.
**Примечание**: Lua покрывает 80% use-cases для скриптинга; Java — для совместимости
с Java-модами OC2R (если понадобится).

### Полноценный GUI (v0.7+, ~1 месяц)
- [ ] Window manager (создание/удаление окон)
- [ ] Compositor (z-order, clipping)
- [ ] Mouse cursor + click handling
- [ ] Widget toolkit (advanced: ScrollView, Menu, Dialog)

### Безопасность / userland:
- [x] **umask/права OnyxFS** (2026-09-01): добавлено поле `Proc::umask` (дефолт
      0o022, наследуется через fork), syscall `umask()` (#88,
      `kernel/src/syscall/fs_sys3/info.rs`), применяется к `mode` при
      `O_CREAT` в `sys_open` (`kernel/src/syscall/fs_sys/open_close/open.rs`).
      Аудит показал, что `/etc/shadow` уже был защищён общей проверкой
      permission bits (резолвится и для symlink через `lookup()`), хардкод
      пути — просто defense-in-depth, реального обхода не было; hardlink как
      примитив в OnyxFS не реализован.
- [ ] $5$-хэш совместимость с crypt(3) — текущая схема (`core/src/crypto/kdf.rs`)
      сознательно НЕ совместима с glibc sha256crypt (hex вместо crypt-base64,
      фиксированные 10k раундов вместо digest A/B/DP/DS алгоритма Дреппера);
      полная реализация — отдельная многочасовая задача с изменением формата
      хранения и миграцией.
- [x] **`passwd` с пустым текущим паролем** (2026-09-01): политика явно
      задокументирована и покрыта тестом — пустое/`*`/`!` поле shadow это
      **locked account**, не "любой пароль подходит" (в отличие от
      классического crypt(3)); см. `parse_shadow_field` в
      `core/src/crypto/kdf.rs` и тест `locked_account_fields_fail_closed`.

### Платформа / время:
- [ ] RTC под sedna (gettimeofday от реального времени)
- [ ] nanosleep точность (SBI set_timer vs CLINT)
- [ ] SBI-звонки (get_spec_version, reboot/shutdown через SRST)

### Ввод / QoL:
- [ ] Ctrl+D = EOF в cooked-read
- [ ] backspace/стрелки в raw-режиме
- [ ] osh история + tab-completion
- [ ] UART IRQ-driven rx (PLIC-регистрация)

### Тесты:
- [~] journal crash-recovery с реальным блочным I/O (ручной QEMU-цикл)

## ✅ Найдено и ИСПРАВЛЕНО (2026-08-29):
- [x] xHCI init: MaxScratchpad читался из HCSPARAMS1 → HCSPARAMS2
- [x] virtio-blk сериализация: per-device SpinLock G_QLOCK
- [x] font UAF: буфер намеренно не освобождается (leak ограничен)
- [x] SAFETY-комментарии волна 2: fs/, syscall/, net/, proc/ (~1200 строк)

## 🤝 Принятые компромиссы (не баги):
- lto=false: fat/thin ломают линк ядра (__rust_alloc после LTO-merge)
- KDF без memory-hardness — принятая позиция (10k SHA-256)
- Точечные clippy-исключения (34 allow) — обоснованы в коде
- Правило 2 остатки (fdt 13, onyxfs 12 файлов) — осознанно
- onyx_init бины не собираются под host-тесты (сырой RISC-V asm)
