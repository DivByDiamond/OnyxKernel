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

## ❌ КРИТИЧНО ДО 15 СЕНТЯБРЯ:

### 🔥 Приоритет 1 — Критические гонки и баги (БЛОКЕР СТАБИЛЬНОСТИ!)
**Владелец: GLM** — объёмная работа, требует глубокого понимания concurrency
**Статус: ✅ ВЫПОЛНЕНО 2026-08-31** (см. docs/CONCURRENCY.md, git-история)

- [x] **fork race**: create_user ставит ребёнка Ready+enqueue ДО копирования fds/signal_handlers/
      cwd/mmap_brk/tf (fs_sys3/extra/exec/fork.rs) — work-stealing харт может забрать
      ребёнка с нулевыми fd.
      **Фикс (2026-08-31)**: новое ProcState::Creating; create_user больше не enqueue-ит;
      sys_fork копирует всё состояние, затем атомарная публикация proc::publish_ready
      (Creating→Ready+enqueue под PROC_LIST_LOCK, rq_lock внутри).

- [x] **waitpid data race**: sys_waitpid пишет state=Waiting вне proc_list_lock (exec/mod.rs:115);
      proc::dump_all тоже итерирует G_ALL_PROCS без лока.
      **Фикс (2026-08-31)**: Waiting публикуется в той же крит-секции, что и has_child
      (протокол B4); dump_all/count() обходят список под try_lock (panic-путь деградирует
      в best-effort, а не дедлок).

- [x] **sched_setaffinity UAF**: p.affinity пишется без лока, by_pid может вернуть Exited
      proc — узкое окно UAF с конкурентным waitpid.
      **Фикс (2026-08-31)**: lookup+проверка Exited+запись affinity целиком под
      proc_list_lock (by_pid_unlocked); Exited → ESRCH; getaffinity — так же.

- [x] **Сеть без синхронизации**: UDP_SOCKS/CONNS/ARP cache/IP_ID/NEXT_PORT/VX rings
      мутируются из syscall-пути (net_sys.rs) без лока между хартами.
      **Фикс (2026-08-31)**: рекурсивный NET_LOCK (net/lock.rs: owner+depth) вокруг всех
      pub-функций udp/tcp/arp/ip, poll и RX-обработчиков; udp_recv/udp_close получили
      bounds-check индексов (раньше паниковали).

- [x] **procfs G_PROCBUF race**: shared static mut scratch-буфер без лока (procfs/content.rs).
      **Фикс (2026-08-31)**: per-hart G_PROCBUF_HARTS[MAX_HARTS][PROCFS_MAX_SIZE],
      индекс hart_id (маскирован) — нулевая контенция, SIE=0 покрывает same-hart.

- [x] **chown без проверки владельца**: любой процесс может chown любой файл (vfs/meta/chown.rs).
      **Фикс (2026-08-31)**: chown_allowed() (uid==owner || uid==0 || ring<=ROOT), EPERM
      иначе; bypass is_kernel_boot как в chmod; применяется в chown и fchown (stat по ino).

- [x] **libfdt bounds checking**: walk читает токены за границей; is_sedna/is_qemu сканируют
      256KiB без totalsize-бонда; cstr_at может уйти за strings block.
      **Фикс (2026-08-31)**: init_from валидирует offset/size против totalsize (cap 4MiB);
      walk ограничивает токены/имена/prop-data концом блока; cstr_at ограничен
      strings-блоком; is_sedna/is_qemu сканируют min(totalsize, 256KiB).

**Тесты (2026-08-31)**: хост 131/131 (`cargo test -p onyx_kernel`: proc fork-publish
инварианты, net table exclusivity + NET_LOCK рекурсия, chown policy, fdt malformed-DTB);
`cargo test -p onyx_core` зелёный; kbuild/kbuild32/smode собираются; clippy strict чистый;
QEMU SMP 2 и 4: полный boot → login → fork-стресс фоновыми задачами → procfs → 0 паников
(`scripts/test_concurrency.sh`).

### 🎮 Приоритет 2 — Lua runtime для OC2R
**Владелец: boba** — быстрая реализация, видимый результат

- [ ] **Lua VM минимальный** (~500-1000 строк):
      1. Stack machine (push/pop, базовые операции)
      2. Tables (hash map для userdata)
      3. Upvalues (closures)
      4. Basic libs: string, table, math
      5. syscall bindings (open/read/write/close)

- [ ] **Lua REPL в /bin/lua**:
      - Интерактивный режим
      - Загрузка .lua файлов
      - Error handling

- [ ] **Примеры для демо**:
      - hello.lua
      - fs_explorer.lua (листинг /proc)
      - simple_calc.lua

**Формат**: userspace программа в ring2, без изменений ядра.
**Референс**: посмотри PUC-Rio Lua 5.1 (самая простая версия), ~15k строк C.

### 🖥️ Приоритет 3 — TUI базовый фундамент
**Владелец: boba** — параллельно с Lua

- [ ] **Mouse syscall**: SYS_mouse_read (event: x, y, buttons)
- [ ] **Double buffering**: fb::swap_buffers() для устранения tearing
- [ ] **Event loop**: kernel/src/srv/event.rs — poll клавиатуры/мыши
- [ ] **Basic TUI lib** (userspace, /lib/tui.lua для Lua или /lib/libtui.a для C):
      - Widget trait (draw, handle_event)
      - Button, Label, TextBox
      - Layout (horizontal/vertical stack)

**Демо**: /bin/tui_demo — 3 кнопки + текстовое поле.

### 🔌 Приоритет 4 — OC2R интеграция
**Владелец: оба** — последняя неделя

- [ ] Проверка загрузки через OnyxOSFirmware блок
- [ ] Сеть: DHCP вместо хардкода IP (используй существующий UDP стек)
- [ ] Framebuffer: тест r5g6b5 режима на OC2R мониторе
- [ ] Snapshot на несъёмном диске OC2R

## 📅 ПОСЛЕ 15 СЕНТЯБРЯ (v0.6+):

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
- [ ] umask/права OnyxFS — не-root не может open /etc/shadow
- [ ] $5$-хэш совместимость с crypt(3)
- [ ] `passwd` с пустым текущим паролем

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
