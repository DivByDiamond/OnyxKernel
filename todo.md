# OnyxKernel — TODO

## ✅ Готово:
1. **Полный рерайт на Rust** (~98%, assembly через global_asm!)
2. **Динамические процессы** — нет PROC_MAX, heap-allocated linked list
3. **OnyxExec v2** — dynamic segments (до 256), ring1 flag, compression flag
4. **OnyxFS v2** — timestamps (crtime/mtime/atime/ctime), indirect blocks, dirents 40 bytes
5. **Flashback snapshots** — snapshot_create / rollback / list с RLE сжатием + COW data blocks
6. **Root/User Space** — 3 ring'а, syscall ACL, path-policy, dropring
7. **Syscalls (77)** — полная таблица ядерных вызовов (v0.4 — userspace-ready update):
   - **1-5**: write, read, exit, yield, getpid
   - **6-7**: brk, mmap ✅ (раньше были stubbed)
   - **8-13**: open, close, lseek, stat, exec, sbrk
   - **14-18**: spawn, wait, readdir, getring, dropring
   - **19-23**: snapshot_create/rollback/list, kill, sigmask
   - **24-26**: write_fd, create, mkdir
   - **27-33**: chan_create/connect/send/recv/close/create_named/open
   - **34-36**: munmap, dup, pipe (NEW)
   - **37-40**: unlink, rename, chdir, getcwd (NEW)
   - **41-44**: truncate, access, gettimeofday, fcntl (NEW)
   - **45-48**: getuid, getgid, utimens, uname (NEW)
   - **49**: nanosleep (NEW)
   - 🐛 **Fix**: SYS_chan_open(33) был пропущен в ACL — теперь доступен user-пространству

   **v0.4 additions (50–77):**
   - **50**: `fstat(fd, struct stat *)` — POSIX-style, fills Linux-compatible 128-byte struct stat
   - **51**: `waitpid(pid, *status, options)` — wait for specific child, supports WNOHANG
   - **52**: `getdents64(fd, buf, len)` — batched directory reads (stub)
   - **53**: `ioctl(fd, req, arg)` — terminal control (TCGETS/TCSETS/TIOCGWINSZ/FIONREAD)
   - **54**: `mprotect(addr, len, prot)` — change page protections
   - **55**: `sigaction(signum, *act, *oldact)` — install user-space signal handlers
   - **56**: `sigprocmask(how, *set, *oldset)` — block/unblock signals
   - **57**: `sigreturn()` — restore trap frame after handler
   - **58**: `execve(path, argv, envp)` — exec with environment variables
   - **59**: `getppid()` — return parent PID
   - **60-62**: `setpgid`, `setsid`, `getpgid` — process group management (stubs)
   - **63**: `fork()` — vfork-style; child shares parent's address space until exec
   - **64-65**: `clock_gettime`, `clock_getres` — POSIX clocks (REALTIME/MONOTONIC)
   - **66**: `isatty(fd)` — terminal detection
   - **67**: `getentropy(buf, len)` — up to 256 bytes of xorshift64 entropy
   - **68-69**: `setuid`, `setgid` — identity change (root-only)
   - **70**: `fsync(fd)` — flush to disk (no-op; OnyxFS writes through immediately)
   - **71**: `truncate2(path, len)` — POSIX truncate with explicit length
   - **72**: `ftruncate(fd, len)` — same for fd
   - **73-74**: `readlink`, `symlink` — symbolic links (stubs — OnyxFS has no symlinks yet)
   - **75-76**: `chmod`, `fchmod` — permission bits (no-op; OnyxFS has no perms yet)
   - **77**: `getdents` — old-style compat alias for getdents64
8. **OnyxFS write** — onyxfs_write(), create(), mkdir() с bitmap allocation
9. **Journal recovery** — write-ahead journal + recovery при mount
10. **I/O batching** — read_multi/write_multi для multi-sector I/O
11. **Preemption** — timer tick → sched_tick → NEED_RESCHED → sched_yield
12. **Блокирующий wait** — Waiting state + sched_yield
13. **Signal delivery** — SYS_kill, SIGKILL terminates
14. **Рефакторинг** — все файлы ≤150 строк
15. **QEMU verified** — ядро грузится, init работает в ring 1
16. **onx::load BSS page-fault fix** — `PTE_A | PTE_D` теперь выставляются для всех
    user-leaf PTE в сегментах / стеке / куче (раньше `map_one_pub` вызывался
    без A/D, что под QEMU с `menvcfg.ADUE = 0` приводило к page fault на
    первом обращении — типичный симптом: `onyxcc` падал на доступе к BSS
    по адресу `0x199f0`, где располагается первый глобал 1.2 MB сегмента).
17. **Unicode таблица в PSF1/PSF2** — glyph → unicode mapping, glyph_for_unicode(),
    glyph_bitmap_unicode(), UTF-8 декодирование и рендеринг в framebuffer
18. **IPC channels** — ipc::channel с create/create_named/open_by_name/connect/send/recv/close,
    блокирующий wait, ring buffer 4KB, up to 32 channels
19. **`/ipc/*` VFS** — ipcfs модуль: lookup/stat/read/write/readdir, mounted at /ipc
20. **FDT parser** — libfdt::fdt с полным DTB walk, find_memory/find_plic/find_clint/find_uart/find_virtio/model
21. **PLIC IRQ dispatch** — register_handler/dispatch, up to 64 IRQ handlers
22. **Framebuffer драйвер** — 32bpp, PSF1/PSF2, draw_char/draw_str/scroll/fb_term
23. **SMP (multi-core)** — secondary hart boot, per-hart current proc, scheduler spinlock,
    secondary harts enter idle→scheduler loop
24. **Panic recovery (kdump)** — stack trace (frame pointer walk), process list dump,
    QEMU reboot via test finisher

## ❌ Осталось сделать:

### Приоритет 1 — Userland:
- [x] **`/bin/login`** — аутентификация (root + пользователи из /etc/passwd), dropring(USER), exec(/bin/osh)
- [x] **`/bin/osh`** — пользовательский shell (ring 2) с командами ls/cat/echo/exec/clear/exit
- [x] **`/bin/passwd`** — смена пароля (root + self)
- [x] **`/bin/useradd`** — добавление пользователя (root only)
- [x] **`/bin/userdel`** — удаление пользователя (root only)
- [x] **`/etc/passwd`** + `/etc/shadow` — парсинг, аутентификация
- [x] **`/users/`** — домашние директории пользователей (/users/username/)
- [x] **Per-process FD table** — уже сделан (per-process VfsFd в Proc)
- [x] **add_dirent overwrite** — create теперь перезаписывает существующий dirent (вместо дублирования)
- [x] **First-boot setup** — нет дефолтных паролей; login запрашивает пароль root при первом запуске
- [x] **mkimage --add/--add-dir** — рекурсивное добавление директорий и отдельных файлов

### Приоритет 2 — /proc/ файловая система:
- [x] **procfs** — виртуальная ФС с информацией о системе

### Приоритет 3 — /font/ и шрифты:
- [x] **psfgen** + **PSF1/PSF2 парсер** + загрузка `/font/default.psf`
- [x] **Поддержка Unicode таблицы** — `glyph_for_cp()`, `glyph_or_default()`, psfgen mode=0x02

### Приоритет 4 — IPC:
- [x] **IPC channels** — chan_create/connect/send/recv для root↔user коммуникации
- [x] **`/ipc/*` виртуальный путь** в VFS через ipcfs (mount, lookup, readdir)

### Приоритет 5 — Драйверы:
- [x] **SDHCI драйвер** — для Milk-V Duo S (6→4 файла, ≤200 строк каждый)

### Приоритет 6 — Инструменты:
- [x] **elf2onx v2** — v2 формат с compressed_size (RLE сжатие сегментов + флаг ONX_FLAGS_COMPRESSED)
- [x] **mkimage v2** — v2 образы с snapshot area + journal (уже было реализовано)

### Приоритет 7 — Общее:
- [ ] **OC2R-блок «Загрузчик ОС»** (см. oc2r/todo.md секция 30): игрок кладёт флешку/диск в блок, указывает путь к образу (`config/oc2r/onyx-kernel.bin`, `config/oc2r/onyxfs.img`) → получает предмет с прошитой OnyxOS. Не требует пересборки мода и прав на сервер.
- [ ] Проверить, что кастомный kernel из `config/oc2r/onyx-kernel.bin` грузится (OnyxOSFirmware уже читает override с fallback на jar — коммит `0b90b3b` в oc2r).
- [ ] Проверить, что кастомный rootfs из `config/oc2r/onyxfs.img` маунтится (OnyxOSBlockDeviceData уже читает override).
- [ ] Сеть в OC2R: OnyxOS подхватывает адрес из FDT/DHCP, а не хардкод `[10,0,2,15]`.
- [ ] GPU/framebuffer: формат `r5g6b5` на мониторе OC2R — проверить отрисовку PSF-шрифтов.
- [x] **Panic recovery** — kdump (CSR, backtrace, hartid, dump_all), QEMU reboot
- [x] **Multi-core (SMP)** — G_HART_CURRENT, G_HART_IDLE_TF, SpinLock, sched_enter_idle()
- [x] **RLE decompression в загрузчике onx** — распаковка сжатых сегментов при загрузке
- [x] **SMP scheduler improvements** — per-CPU run queues, per-CPU need_resched, enqueue/dequeue API
- [x] **Load balancing** — steal from remote CPU when local queue empty (pull model, try_lock, race-safe)
- [x] **CPU affinity syscall** — `sched_setaffinity` / `sched_getaffinity` (SYS 78/79) + affinity-aware steal + redirect on dequeue

---

## OC2R/sedna — интеграционное TODO (2026-08-23, после первого успешного логина в игре)

### ✅ Уже сделано в рамках интеграции:
- [x] **`boot_smode`** — вход из OpenSBI в S-mode (a0=hartid, a1=DTB), работает в OC2R
- [x] **Framebuffer монитора** — `/chosen/simple-framebuffer` из FDT: MMIO r5g6b5 16bpp,
      динамическая геометрия (width/height/stride), приоритет над RAM-fallback
- [x] **Login CR-фикс** — терминал OC2R шлёт Enter как `\r`; login больше не ломает пароль хвостовым CR
- [x] **Пустой пароль root** — seed при первом буте + login принимает голый Enter

### Дисплей / framebuffer:
- [ ] **Скорость отрисовки по MMIO**: `put_pixel` пишет по байтам/слову на каждый пиксель глифа;
      на 1920×1080 (GPU T4) баннер и консоль будут ползти. Нужны: блочные копии (word/bulk),
      кэш строки, dirty-строки вместо полного redraw
- [ ] **`scroll()` побайтовый** — над MMIO это сотни тысяч одиночных store; сделать копирование
      машинными словами с учётом выравнивания и хвоста
- [ ] **Tearing/двоение кадра**: хост сэмплирует MMIO асинхронно; рассмотреть двойную буферизацию
      в RAM + копирование одним проходом, или хотя бы vsync-подобный «рисуем в offscreen, свипаем»
- [ ] **Цвета fb_term для 16bpp** — COL_GREEN/COL_BLACK проходят через put_pixel-конверсию,
      но палитру консоли (ANSI 16 цветов в fb_term) привести к единому виду с UART-терминалом
- [ ] **Несколько simple-framebuffer нод** — монитор + проектор дают две ноды; сейчас берётся первая
      попавшаяся. Добавить выбор через `/chosen/bootargs` (`console=fb0`, `fb=addr`)
- [ ] **`/chosen/bootargs` парсинг** — его вообще нет: loglevel, console=, fb=, root=
- [ ] **Hot-swap разрешения** — игрок сменил GPU → мягкий рестарт VM → новое ядро получит новую
      геометрию из FDT; проверить, что ничего не кэширует старый размер (G_FB пересоздаётся — ок,
      проверить fb_term cursor clamp)
- [ ] **mmap /dev/fb0 в userspace** — прямая отрисовка из процессов (игры/демки): сейчас fb только
      из ядра; нужны uncached PTE на MMIO-регион и ioctl с экспозицией адреса

### Ввод (UART/клавиатура):
- [ ] **Ctrl+C → SIGINT foreground-процессу** — терминал шлёт ETX (0x03); ядро должно доставлять
      сигнал активной задаче, а osh — реагировать (сейчас SIGINT до процесса не доходит?)
- [ ] **Ctrl+D = EOF** в cooked-read (login/osh читают до \n; полудуплексные правила)
- [ ] **Backspace/стрелки в raw-режиме** — пароль вводится вслепую, редактирование невозможно;
      для cooked-режима проверить erasure (\b → затирание в line discipline)
- [ ] **Потеря ввода при burst** — UART 16550A: включить FIFO + IRQ-driven rx вместо polling,
      иначе быстрый вставка (>16 байт) теряет символы
- [ ] **DECCKM/escape-последовательности в osh** — история команд и автодополнение хотя бы по tab

### Блоки / ФС:
- [ ] **Нумерация дисков sedna ≠ QEMU** (vda=bootfs, vdb=rootfs, vdc=HDD игрока):
      `[init] /dev/blk0: MBR signature NOT found` — init ожидает MBR на blk0; просканировать все
      блочные устройства и находить OnyxFS по сигнатуре, а не по индексу
- [ ] **Мусорный указатель в логе** — `sys_open: called path=12670` / `path=3fffee28`: трейс печатает
      сырой указатель до разыменования; оставить только `path_bytes=` строку (или печатать %s безопасно)
- [ ] **OnyxFS journal recovery под реальным диском** — journal/recovery писались под образ mkimage;
      прогнать циклы: запись → «выдернули питание» (останов VM) → перезагрузка → fsck/rollback
- [ ] **Snapshot/Flashback на несъёмном диске OC2R** — rollback при том, что диск общий с хостом
- [ ] **Рост раздела**: образ onyxfs.img фиксированный (~2.4 MB); HDD игрока может быть 128 MB —
      поддержать расширение ФС на свободное место при первом маунте

### Сеть:
- [ ] **Убрать хардкод IP [10,0,2,15]** — DHCP-клиент (минимальный: DISCOVER/OFFER/REQUEST/ACK)
      или чтение адреса из FDT/chosen
- [ ] **virtio-net под sedna** — драйвер есть, но device discovery идёт по захардкоженным базами
      QEMU-virt (0x10001000..); перейти на FDT walk (find_virtio уже есть — использовать для net)
- [ ] **DNS-резолвер** в userland (хотя бы /etc/hosts + UDP:53 без кэша)
- [ ] **ping/ifconfig утилиты** для диагностики прямо в игре

### Время / энтропия:
- [ ] **RTC под sedna** — какой узел в FDT, реализован ли gettimeofday от реального времени
      (иначе timestamps OnyxFS бессмысленны между перезапусками мира)
- [ ] **getentropy** — сейчас xorshift (предсказуем!); подключить virtio-rng, если мод даёт,
      иначе seed от таймера+hartid+cycle как минимум; это критично для salt'ов паролей
- [ ] **nanosleep точность** — SBI set_timer vs CLINT на sedna; проверить drift

### Платформа / SMP:
- [ ] **Сколько хартов поднимает sedna** — если >1, проверить secondary boot путь в S-mode
      (OpenSBI передаёт все харты в ОС? или надо держать их в WFI самому)
- [ ] **SBI-звонки**: sbi_get_spec_version и консольный putchar fallback — убедиться, что
      fw_jump из oc2r (0.0.x buildroot) поддерживает нужные legacy/ext вызовы
- [ ] **Reboot/shutdown** — SBI SRST: `reboot`/`poweroff` в osh должны корректно останавливать
      VM в игре (мод видит board.isRunning=false)
- [ ] **Watchdog/платформенные драйверы Milk-V** — убедиться, что probe на sedna безопасно
      отваливается (не читает чужие MMIO)

### Безопасность / userland:
- [ ] **$5$-хэш несовместим с crypt(3)** — свой SHA256-формат shadow; либо документировать,
      либо перейти на стандартный формат ($5$ rounds=…), чтобы образы были переносимы
- [ ] **`passwd` с пустым текущим паролем** — после перевода root на пустой пароль проверить
      смену пароля (verify_old_password должен принимать пустой, если stored пустой)
- [ ] **umask/права OnyxFS** — chmod заглушками; любой пользователь читает /etc/shadow?
      Минимум: запретить read shadow не-root на уровне VFS ACL
- [ ] **argv/envp в execve** — osh получает аргументы? login exec'ает без них; прокинуть HOME/USER/PATH
- [ ] **Лимиты процессов/памяти** — fork-bomb в игре повесит VM-поток сервера; добавить cap на
      число процессов и квоту памяти на пользователя

### Диагностика / QoL:
- [ ] **klog через простой UART-буфер** — kinf-спам замедляет бут; уровни логирования +
      фильтрация через bootargs
- [ ] **kdump на экран** — при панике дублировать stack trace в framebuffer (если он есть),
      чтобы игрок увидел краш без доступа к серверным логам
- [ ] **Автосмок-тест OC2R** — headless-скрипт (expect по UART): бут → login root/Enter → osh →
      ls /bin → poweroff; гонять в CI рядом с QEMU-тестом
- [ ] **Версионирование образа** — прошивать в onyxfs.img файл `/etc/onyx-version`
      (git hash + дата), чтобы в игре было видно, чем собран диск
