# OnyxKernel — TODO

## 📏 Правила проекта (обязательны для всего кода и всех агентов):

1. **Максимум 250 строк на файл** — файл перерос → дробить по ответственности.
2. **Максимум 4 файла на папку** — одна папка = одна задача/подсистема;
   в идеале один файл = одна ответственность. Перебор → раскидывать по подпапкам.
3. **KISS / DRY / SOLID**:
   - KISS — минимальное рабочее решение, без преждевременных абстракций;
   - DRY — дубли логики выносить в общие функции (в т.ч. между kernel/core/init);
   - SOLID — особенно SRP: модуль/файл меняется по одной причине.
4. **Раскидка кода по подпапкам → сразу запускать сабагента на аудит всего проекта**
   (структура, границы модулей, зависимости, нарушения правил 1–3).
5. **Аудит-агент обязан удалять все `#[allow(dead_code)]` и прочие заглушочные allow**
   (`unused_*`, точечные clippy-exceptions) по всему проекту: мёртвый код — удалить,
   живой, но пока не подключённый — подключить или честно пометить TODO с датой.
6. **Java runtime** — отдельная большая цель (запуск Java-программ на OnyxOS);
   см. Приоритет 8 ниже.

## ✅ Готово:
0. **QEMU smoke-инфраструктура + критический баг exit** — scripts/qemu-smoke.sh (headless) и
   qemu-interactive-smoke.sh (login→osh→exit) в OnyxOS; найден и починен древний баг: kstack
   32KB переполнялся execve (~39.5KB) и затирал заголовок Proc → pid=0 → SYS_exit no-op →
   вечный illegal-loop после `exit` из osh. Фикс: KSTACK 64KB, canary-проверка на каждом трапе,
   SYS_exit не возвращается в userspace, init перезапускает /bin/login
1. **Полный рерайт на Rust** (~98%, assembly через global_asm!)
2. **Динамические процессы** — нет PROC_MAX, heap-allocated linked list
3. **OnyxExec v2** — dynamic segments (до 256), ring1 flag, compression flag
4. **OnyxFS v2** — timestamps (crtime/mtime/atime/ctime), indirect blocks, dirents 40 bytes
5. **Flashback snapshots** — snapshot_create / rollback / list с RLE сжатием + COW data blocks
6. **Root/User Space** — 3 ring'а, syscall ACL, path-policy, dropring
7. **Syscalls (77)** — полная таблица ядерных вызовов (v0.4 — userspace-ready update);
   ⚠️ по аудиту 2026-08-24 фактически в abi.rs/dispatch уже **85** (добавились net_connect/send/
   recv/close, chown/fchown, affinity и др.) — секция ниже не догонена:
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
   - **67**: `getentropy(buf, len)` — up to 256 bytes of entropy
     (⚠️ изначально был xorshift от uptime+pid, переведён на hwrand 2026-08-24 —
     см. «Время / энтропия» в OC2R-секции)
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
14. **Рефакторинг** — все файлы ≤250 строк (лимит правила 1 шапки)
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

### Приоритет 8 — Структура / аудит:
- [~] **Полный аудит проекта сабагентом** — первый прогон сделан 2026-08-24 (6 суб-агентов,
      результат = секция «🔴 Аудит ядра» ниже); остался повторный прогон ПОСЛЕ фиксов и
      перекройки структуры по правилам шапки (правило 4)
- [ ] **Зачистка allow-атрибутов** — удалить все оставшиеся `#[allow(dead_code)]`,
      `#[allow(unused_*)]` и точечные clippy-exceptions (сейчас ~7 dead_code в kernel/src,
      заглушочные в init/src/auth не трогали): мёртвое — удалить, живое — подключить
- [ ] **Java runtime для OnyxOS** (большая цель, разбить на этапы):
  1. Минимальный class loader (.class парсер: constant pool, fields, methods)
  2. Интерпретатор байткода JVM (стек, локальные переменные, базовые инструкции)
  3. Подмножество JDK-библиотек (java/lang/Object,String,System,arraycopy…)
  4. GC (mark-sweep над heap объектов), исключения, потоки поверх proc/scheduler
  5. Сборка hello-world javac → запуск onx-бинарём `/bin/jvm` в QEMU/OC2R
  Оценка формата: JVM-интерпретатор как onx-программа в ring1/2, без изменений ядра
  (нужен только mmap + файловый I/O, всё уже есть)

### 🔴 Аудит ядра (2026-08-24, 6 суб-агентов: структура/логика/конкурентность/стиль/тесты/безопасность)

Вердикт: REQUEST CHANGES. Ниже — блокеры и major-находки по убыванию серьёзности.
Полные детали с цитатами и репро — в отчёте аудита; здесь только чеклист фиксов.

План фиксов — волна 1 (4 параллельных агента, зоны не пересекаются):
  A. Syscall memory safety: B1, B2, B3, B8 + mmap_brk rollback + overflow-checks=true
  B. Конкурентность: единый spinlock + assert SIE=0, B4, B5, таймерные гонки,
     by_pid под локом, affinity-enqueue, root_refcount fetch_sub
  C. Сеть + FAT32: B6 (+чистка state-4 DoS, дренаж send_len), B7, FAT16 guard,
     DHCP xid/chaddr + DNS id
  D. Разное: B9 (+ohci unwrap), ONX-loader vaddr+entry, контракт passwd↔ACL,
     tools → зависимость onyx_core, починка cargo test -p onyx_kernel
После волны 1 — обязательный гейт: headless + interactive smoke на объединённом коде.
Отложено: SMP idle/SIE доводка или заморозка (вопрос архитектуры), KDF memory-hardness,
CI-воркфлоу, dead-code зачистка (~25 pub fn), SAFETY-комментарии (974 unsafe / 9 комментов).

#### Блокеры (детерминированные краши/порча данных):
- [ ] **B1: halt ядра из ring-2 невалидным указателем** — user_ptr_ok (syscall/handler/dispatch.rs:14)
      проверяет только диапазон [USER_BASE, USER_TOP), не маппинг; sys_write разыменовывает
      напрямую под SUM (fs_sys/read_write.rs:19); page fault из S-mode → klog::halt() (srv/trap.rs).
      Репро: write(1, 0x30000000, 1). Фикс: постраничная трансляция через vmm::translate_user*
      перед доступом; фолт → убийство процесса, не halt.
- [ ] **B2: getdents64 пишет за границу физического фрейма** — буфер транслируется один раз,
      пишется до count байт (fs_sys3/extra/info.rs:19-60); при buf близко к концу фрейма записи
      уходят в чужие PA (вплоть до page tables). Фикс: перепереводить каждую страницу или
      ограничивать запись концом текущего фрейма.
- [ ] **B3: mmap переполняется до size=0** — page_align_up(n) = (n+0xFFF)&!0xFFF без checked_add
      (fs_sys3/mem/brk.rs:8); при length ∈ [2^64−4095, 2^64−1] → size 0 → Ok(vaddr) без памяти.
      Фикс: n.checked_add(0xFFF).ok_or(Errno::Range)? & !0xFFF.
      Прим.: после включения overflow-checks=true это станет паникой в syscall-пути
      (kdump вместо тихой порчи — лучше, но паника из ring-2 тоже недопустима),
      так что checked_add нужен независимо.
- [ ] **B4: lost wakeup wait/exit** — wait() ставит state=Waiting ВНЕ proc_list_lock
      (spawn/wait.rs:53), exit проверяет Waiting тоже вне лока (lifecycle/exit.rs:44);
      гонка → родитель спит навечно. Плюс burst-апдейт exit_code+state без лока → устаревший
      статус на слабой модели RISC-V. Фикс: state+wakeup под одним proc_list_lock.
- [ ] **B5: G_CHANNELS полностью без блокировки** — static mut [Channel; CHAN_MAX]
      (ipc/channel/types.rs:22): lost wakeup recv/send (ringbuf.rs:98-107 vs :73),
      порча ringbuf при двух send'ах с разных harts. Фикс: spinlock на канал.
- [ ] **B6: TCP входящие сегменты не верифицируются** — матч только по локальному порту
      (net/tcp/handle.rs:98-101), src/dst IP:port игнорируются, входящий checksum не считается
      → hijack сессии любым хостом L2. После wrap NEXT_PORT — дубликаты портов.
- [ ] **B7: FAT32 зеркало второй FAT получает нули вместо EOC** — write_fat_entry патчит свой
      буфер, а в buf2 копируется непропатченный сектор (fs/fat32/write.rs:90-101) → FAT#2
      помечает живые кластеры свободными → cross-file corruption после recovery.
- [ ] **B8: sys_brk shrink освобождает занятую страницу кучи** — unmap от невыровненного addr
      (brk.rs:69-71), walk округляет вниз → снимается страница с живыми данными ниже нового brk;
      regrow выдаёт zeroed-страницу → тихая потеря данных. Фикс: page_align_up(addr).
- [ ] **B9: psf2 парсинг шрифта — 7×unwrap подряд** (font/psf2.rs:10-16) — кривой файл →
      kernel panic → abort. Фикс: Result<Font, Errno>. Родственное: ohci/control.rs:46,57,124.

#### Конкурентность (SMP частично активен — вторичные harts онлайн, но бесполезны):
- [ ] **Живая гонка: G_UPTICKS/G_JIFFIES** — RMW на static mut со всех тикающих harts
      (srv/timer.rs:13-14,107-108) → потерянные инкременты, дрейф uptime_us. Фикс: AtomicU64 fetch_add.
- [ ] affinity-enqueue в чужую runqueue без лока жертвы (sched.rs:100-104) — тот же класс,
      что починен в steal(), но здесь пропущен → разорванный free-list очереди.
- [ ] неатомарный декремент root_refcount (exit.rs:27-29) → потеря декремента → UAF page tables
      либо утечка корневой таблицы. Фикс: fetch_sub(1, AcqRel) + сравнение с 1.
- [ ] by_pid() обходит G_ALL_PROCS без proc_list_lock (process/current.rs:53-62; вызовы exit.rs:7,43,
      trap.rs:175) — нарушает собственный контракт globals.rs:30-32.
- [ ] trap_return безусловно гасит SIE (arch/asm/trap_asm.rs:94-99) → вторичные harts замолкают
      после первого тика; инвариант «SIE=0 в kernel» нигде не зафиксирован (assert/комментарий).
      Защита от вытеснения под локом сегодня случайная.
- [ ] пять независимых копий spinlock (pmm/heap/vmm/rq/proc_list), heap-версия без backoff;
      унифицировать в один примитив. G_RELEASE пишется SeqCst, читается read_volatile
      (smp.rs:80-90) — перевести читателя на load(Acquire).

#### Логика / память:
- [ ] mmap_brk продвигается ДО валидации и не откатывается при ошибке (mmap.rs:53-56 vs :74-80)
      → одна неудача = вечный ENOMEM для процесса. Фикс: валидация до продвижения.
- [ ] map_anon rollback учитывает максимум 1024 страниц (vmm/map/mod.rs:58,92-95) → mmap >4 MiB
      при ошибке течёт страницами и бюджетом навсегда. Фикс: Vec или счётчик + обратный unmap.
- [ ] krealloc при old_size==0 читает new_size байт из невалидного источника
      (heap/realloc.rs:14-23). Фикс: old_size==0 && p!=null → Err.
- [ ] G_HEAP.used систематически дрейфует (slab-free не вычитает: alloc.rs:62-64) → wrap статистики.
- [ ] PMM init: цикл reserved-marking мёртвый (pmm/mod.rs:106-116, end_bit всегда 0) — kernel/BSS
      защищены случайно (underflow idx), а не по задумке. pmm::free принимает невыровненный PA.
- [ ] FAT32 mount: num_fats захардкожен как 2 (helpers.rs:132), тип ФС (12/16/32) не проверяется
      → FAT16 диск интерпретируется мусорно, возможен free произвольных кластеров.
- [ ] FAT32 unlink: free_chain ДО tombstone dirent (write.rs:329-334) → I/O error посреди =
      кластеры переиспользуются другим файлом. Поменять порядок. Аналогично create: утечка
      кластера при ошибке поиска слота (write.rs:244 vs :279).
- [ ] ONX-loader: проверяется только s.vaddr, не vaddr+memsz (onx/load.rs:90-92); entry не
      валидируется (:129). Фикс: проверка конца диапазона + entry ∈ [USER_BASE, USER_TOP).
- [ ] header.rs::to_bytes_v2 паникует при segs.len() > nsegs (поля публичны, core/formats/header.rs:127-137).
- [ ] TCP: send_len никогда не дренируется (handle.rs:42-46) → после 2048 байт соединение мертво
      при виде «Ok»; state 4 не чистится без явного close → 8 удалённых FIN = перманентный DoS
      (MAX_CONNS=8); FIN без проверки seq == rcv_nxt.

#### Безопасность:
- [ ] KDF паролей: 10k SHA-256 раундов без memory-hardness; соль молча деградирует до LCG при
      отказе hwrand (auth/crypto/extra.rs:48-63 + drivers/hwrand.rs:44-56) — договор молчания,
      деградацию надо сигналировать вызывающему.
- [ ] Контрактный разрыв: /bin/passwd исполняется в ring 2, create/rename — ring≤1 only
      (handler/acl.rs:76,81) → смену пароля нельзя выполнить даже root'ом.
      → Решение и статус: один канонический пункт «umask/права OnyxFS» в OC2R-секции
      ниже; фикс контракта — волна 1, агент D.
- [ ] DHCP/DNS: xid/chaddr не сверяются (net/dhcp/protocol.rs:85-144); DNS id = uptime_us()
      предсказуем (dns.rs:39) → спуфинг ответов в общем L2.
- [ ] unsafe: 974 вхождения, SAFETY-комментариев 9 (<1%) — проставить SAFETY хотя бы в
      drivers/vmm/trap путях (правило unsafe-safety-comment).

#### Тесты / CI / сборка:
- [ ] **overflow-checks=true для release ядра** — однострочник в workspace Cargo.toml
      ([profile.release]). ✅ РЕШЕНИЕ АВТОРА (2026-08-24): цена небольшая, а «тихая порча»
      при переполнении в ядре хуже паники (kdump/reboot). Волна 1, агент A.
      ⚠️ После включения прогнать QEMU smoke — возможны ранее немые overflow в hot path.
- [ ] cargo test -p onyx_kernel НЕ компилируется (39×E0425 протухшие константы virtio/test.rs)
      — починить или удалить тавтологию assert_eq!(0x74726976, 0x74726976) (virtio/test.rs:6).
- [ ] Known-answer тесты SHA256 (init/src/auth/crypto/sha256.rs) — аутентификация сейчас
      без единого автотеста.
- [ ] Покрыть ACL (syscall_allowed), journal crash-recovery с реальным блочным I/O,
      TCP state machine, IPC ringbuf, scheduler runqueue.
- [ ] CI: добавить cargo fmt --check + cargo clippy -W correctness,suspicious,perf; target
      riscv32imac (заявлен в .cargo/config.toml, в CI нет); MSRV-проверку против плавающего nightly.
- [ ] release-профиль: debug=true + strip="none" раздувает ELF DWARF'ом → strip="debuginfo"
      (unstripped уже аплоудится как artifact); lto="fat" почти бесплатен (deps=0).
- [ ] docs/architecture.md протух: 31 syscall против фактических 85 (abi.rs), битые ссылки на
      handler.rs:38-59 (теперь handler/dispatch.rs); todo.md «Syscalls (77)» выше — фактически 85.
- [ ] Дублирование форматов: tools/mkimage и elf2onx вручную копируют константы/layout из
      onyx_core::formats (magic, DT_REG, SNAPSHOT_BLOCKS_EACH=64 в двух местах) → сделать
      onyx_core зависимостью onyx_tools. Ядро поступает правильно (единственный источник правды).
- [ ] Dead code: ~25 pub fn без ссылок (drivers/pinctrl, rtc, pwm, i2s целиком, vfs dup2,
      lookup_follow...) — связать с правилом 5 шапки (зачистка allow + удаление).
      lookup_follow: симлинки создаются, но никогда не резолвятся — контрактный разрыв open↔symlink.
- [ ] rustfmt.toml/clippy.toml/[workspace.lints] отсутствуют — зафиксировать конфиги до спора.

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
- [x] **`scroll()` побайтовый** — переписан на volatile word-копии (vcopy/vzero с выравниванием
      и хвостом, без memcpy — MMIO); clear() тоже word-zero; 32bpp put_pixel — один u32 store
- [ ] **Tearing/двоение кадра**: хост сэмплирует MMIO асинхронно; рассмотреть двойную буферизацию
      в RAM + копирование одним проходом, или хотя бы vsync-подобный «рисуем в offscreen, свипаем»
- [ ] **Цвета fb_term для 16bpp** — COL_GREEN/COL_BLACK проходят через put_pixel-конверсию,
      но палитру консоли (ANSI 16 цветов в fb_term) привести к единому виду с UART-терминалом
- [ ] **Несколько simple-framebuffer нод** — монитор + проектор дают две ноды; сейчас берётся первая
      попавшаяся. Добавить выбор через `/chosen/bootargs` (`console=fb0`, `fb=addr`)
- [x] **`/chosen/bootargs` парсинг** — srv/bootargs.rs: парс /chosen/bootargs при буте,
      `loglevel=info|warn|err` применяется к klog (фильтр в макросах — отфильтрованные вызовы
      не делают vformat), generic key[=value] accessor bootargs::get() под console=/fb=/root=
- [ ] **Hot-swap разрешения** — игрок сменил GPU → мягкий рестарт VM → новое ядро получит новую
      геометрию из FDT; проверить, что ничего не кэширует старый размер (G_FB пересоздаётся — ок,
      проверить fb_term cursor clamp)
- [ ] **mmap /dev/fb0 в userspace** — прямая отрисовка из процессов (игры/демки): сейчас fb только
      из ядра; нужны uncached PTE на MMIO-регион и ioctl с экспозицией адреса

### Ввод (UART/клавиатура):
- [x] **Ctrl+C → SIGINT foreground-процессу** — SIGINT(2) в proc/signals (default: terminate),
      foreground = последний созданный не-init процесс (G_FG_PID в create_user); 0x03 перехватывается
      в tty::filter_input и доставляется как сигнал. Ограничения: пайплайны убивают только последнего,
      Ctrl+C на пустом промпте no-op (нет process groups)
- [ ] **Ctrl+D = EOF** в cooked-read (login/osh читают до \n; полудуплексные правила)
- [ ] **Backspace/стрелки в raw-режиме** — пароль вводится вслепую, редактирование невозможно;
      для cooked-режима проверить erasure (\b → затирание в line discipline)
- [~] **Потеря ввода при burst** — FIFO включён (FCR 0xC7: enable + trigger 14 байт) в обоих
      режимах; IRQ-driven rx НЕ сделан (PLIC-регистрация для UART отсутствует) — burst >16 байт
      между поллами всё ещё может терять символы
- [ ] **DECCKM/escape-последовательности в osh** — история команд и автодополнение хотя бы по tab

### Блоки / ФС:
- [x] **Нумерация дисков sedna ≠ QEMU** (vda=bootfs, vdb=rootfs, vdc=HDD игрока):
      ядро уже сканировало все virtio-blk в vfs::setup; devfs теперь экспонирует /dev/blk0..blk7
      по числу probe-нутых устройств (devfs/blk.rs, чтение/запись с dev_idx вместо хардкода 0);
      init boottest сканирует все /dev/blkN и ищет ONY2 (LBA 0 или LBA 10240 за MBR)
- [x] **Мусорный указатель в логе** — `sys_open: called path=12670`: первый kinf! с сырым
      указателем удалён, остался только безопасный `path_bytes=%s` после parse_user_path
- [~] **OnyxFS journal recovery под реальным диском** — логика scan/replay вынесена в чистые
      функции onyx_core::formats (43 хост-теста: clean journal, commit, torn tail, чужой magic);
      железный цикл «запись → kill VM → reboot» на OC2R всё ещё предстоит прогнать руками
- [ ] **Snapshot/Flashback на несъёмном диске OC2R** — rollback при том, что диск общий с хостом
- [x] **Рост раздела**: grow-on-mount в onyxfs/mount.rs — если диск больше superblock.total_blocks,
      ФС расширяется до размера устройства при первом маунте (суперблок персистится сразу).
      Капы: single-block data bitmap → 32768 блоков (~128 MiB данных) + ONYFS_MAX_TOTAL_BLOCKS 1 GiB;
      биты bitmap за концом образа уже нули у mkimage, данные идут хвостом — совместимо со старыми образами

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
- [x] **getentropy** — sys_getentropy теперь через hwrand::fill (Zkr seed CSR → virtio-rng →
      LCG от RTC^cycle); источник логируется при буте; Zkr оставлен заглушкой осознанно
      (illegal-instruction без recovery в trap-хендлере, см. drivers/hwrand.rs)
- [x] **umask/права OnyxFS** — ✅ КАНОНический пункт по passwd↔shadow↔ACL (другие места
      ссылаются сюда):
      1) не-root запретён любой open /etc/shadow на уровне sys_open (uid==0 / ring<=1 обходят);
      2) ⚠️ известный tradeoff: ring-2 `passwd` (self-service) тоже читает shadow целиком и
         получает EPERM — самосмена пароля сломана осознанно, пока меняем пароль через root;
      3) контрактный разрыв ACL: create/rename — ring≤1 only, а /bin/passwd живёт в ring 2
         (handler/acl.rs:76,81), т.е. смена пароля недоступна даже root'ом из osh;
      4) полное решение (волна 1, агент D): setuid-хелпер или per-user shadow
         (/etc/shadow/<user>) + правка ACL — закрывает пункты 2 и 3 одновременно; chmod
         остаётся заглушкой до отдельной задачи
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
- [x] **umask/права OnyxFS** — см. канонический пункт выше («Время / энтропия»)
- [x] **argv/envp в execve** — sys_execve копирует user char** (лимиты 32 записи/256 байт),
      стек инициализируется по RISC-V ABI (argc/argv/envp); login передаёт argv=["osh"] и
      envp HOME/USER/SHELL/PATH=/bin
- [x] **Лимиты процессов/памяти** — proc/limits.rs: 128 procs глобально, 32/uid (root 256) → EAGAIN;
      128 MiB системный бюджет user-pages → ENOMEM (учёт в brk/mmap/onx-load/unmap/exit);
      попутно закрыты утечки страниц при ошибках spawn/load/map_anon

### Диагностика / QoL:
- [x] **klog уровни + фильтрация через bootargs** — см. пункт `/chosen/bootargs` выше;
      panic-вывод фильтруется всегда
- [ ] **kdump на экран** — при панике дублировать stack trace в framebuffer (если он есть),
      чтобы игрок увидел краш без доступа к серверным логам
- [ ] **Автосмок-тест OC2R** — headless-скрипт (expect по UART): бут → login root/Enter → osh →
      ls /bin → poweroff; гонять в CI рядом с QEMU-тестом
- [ ] **Версионирование образа** — прошивать в onyxfs.img файл `/etc/onyx-version`
      (git hash + дата), чтобы в игре было видно, чем собран диск
