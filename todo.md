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
