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
6. **Java runtime** — отдельная большая цель (см. ниже).

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

## ❌ Осталось сделать:

### Найдено 2026-08-29 (верификация SAFETY-комментариев):
- [x] xHCI init: MaxScratchpad читался из HCSPARAMS1 (MaxPorts!) → исправлено на
      HCSPARAMS2: MSB[31:27]<<5 | LSB[8:4] (первая попытка с bits [27:4] была неверной —
      поймал верификатор) + guard 1+i < max_slots+1 (xhci/init.rs). QEMU virt HCC_SPS=0.
- [x] virtio-blk сериализация: per-device SpinLock G_QLOCK (drivers/virtio/mod.rs);
      request() держит лок на весь жизненный цикл (copy-in, submit, poll, copy-out);
      read_multi/write_multi не рекурсивны (лок берётся per-sector). API не изменён.
- [x] font UAF (находка волны 2): srv load_font делал kfree буфера, на который
      font::init оставил указатели (G_FONT.glyphs/unicode) → каждый рендер глифа
      читал освобождённую память. Фикс: буфер намеренно не освобождается (leak
      ограничен размером шрифта, blob живёт до конца работы ядра).
- [x] SAFETY-комментарии, волна 2: fs/, syscall/, net/, proc/, ipc/, font/, libfdt/,
      srv/ — ~1200 строк комментариев, 0 непокрытых unsafe-блоков во всём крейте
      (grep-аудит). ringbuf.rs ужат до 250 (компакт # Safety-доки).
- [ ] **Реальные гонки/дыры, найденные при волне 2 (не фикшено — фиксить волне 3):**
      1) fork: create_user ставит ребёнка Ready+enqueue ДО копирования fds/signal_handlers/
         cwd/mmap_brk/tf (fs_sys3/extra/exec/fork.rs) — work-stealing харт может забрать
         ребёнка с нулевыми fd.
      2) sys_waitpid пишет state=Waiting вне proc_list_lock (exec/mod.rs:115) — формальный
         data race; proc::dump_all тоже итерирует G_ALL_PROCS без лока.
      3) sys_sched_setaffinity: p.affinity пишется без лока, by_pid может вернуть Exited
         proc — узкое окно UAF с конкурентным waitpid.
      4) Сеть: нет синхронизации между хартами вообще — UDP_SOCKS/CONNS/ARP cache/
         IP_ID/NEXT_PORT/VX rings мутируются из syscall-пути (net_sys.rs) без лока.
      5) procfs: G_PROCBUF — shared static mut scratch-буфер без лока (procfs/content.rs).
      6) chown/fchown: нет проверки владельца (любой процесс может chown любой файл —
         vfs/meta/chown.rs + owner.rs).
      7) libfdt: walk читает токены за границей struct-block на кривом DTB; is_sedna/
         is_qemu сканируют 256KiB за G_DTB без totalsize-бонда; cstr_at может уйти за
         strings block (reader.rs/walk.rs/model.rs).
      8) ip::send_packet вызывает net::poll() изнутри RX-dispatch (reentrancy) — учесть
         при добавлении локов в сеть.
      9) klog::panic_handler зовёт kdump без восстановления его SIE-контракта
         (диагностика только volatile/CSR — косметика).

### Тесты:
- [~] Осталось из плана покрытия: journal crash-recovery с реальным блочным I/O
      (ручной QEMU-цикл «запись → kill VM → reboot»; чистая логика уже в onyx_core, 43 теста).
      ACL, TCP state machine, IPC ringbuf, runqueue — уже покрыты (121 тест onyx_kernel).

### Приоритет 7 — OC2R-интеграция:
- [ ] Блок «Загрузчик ОС» (см. oc2r/todo.md секция 30): флешка/диск в блоке + путь к образу
      (`config/oc2r/onyx-kernel.bin`, `config/oc2r/onyxfs.img`) → предмет с прошитой OnyxOS.
- [ ] Проверить кастомный kernel (OnyxOSFirmware читает override, коммит `0b90b3b` в oc2r)
      и кастомный rootfs (OnyxOSBlockDeviceData) из config/oc2r.
- [ ] Сеть: адрес из FDT/DHCP вместо хардкода `[10,0,2,15]`; virtio-net discovery через
      FDT walk (find_virtio) вместо захардкоженных баз; DNS-резолвер (/etc/hosts + UDP:53);
      ping/ifconfig утилиты.
- [ ] GPU/framebuffer: r5g6b5 на мониторе OC2R — проверить отрисовку PSF-шрифтов;
      скорость MMIO-отрисовки (блочные копии, dirty-строки); tearing (двойная буферизация);
      16bpp-палитра fb_term; несколько simple-framebuffer нод (монитор+проектор);
      hot-swap разрешения; mmap /dev/fb0 в userspace.
- [ ] Snapshot/Flashback на несъёмном диске OC2R (диск общий с хостом).

### Приоритет 8 — Ввод / QoL:
- [ ] Ctrl+D = EOF в cooked-read; backspace/стрелки в raw-режиме (пароль вводится вслепую);
      erasure (\b) в line discipline; DECCKM/escape в osh — история + tab-completion.
- [ ] UART IRQ-driven rx (PLIC-регистрация отсутствует; burst >16 байт может терять символы).
- [ ] kdump на экран (stack trace в framebuffer); автосмок-тест OC2R (expect по UART) в CI;
      версионирование образа (/etc/onyx-version: git hash + дата).

### Платформа / время:
- [ ] RTC под sedna (gettimeofday от реального времени — иначе timestamps между
      перезапусками мира бессмысленны); nanosleep точность (SBI set_timer vs CLINT).
- [ ] Сколько хартов поднимает sedna (>1 → проверить secondary boot в S-mode);
      SBI-звонки (get_spec_version, putchar fallback у fw_jump 0.0.x); reboot/shutdown
      через SBI SRST; безопасный probe платформенных драйверов Milk-V.

### Безопасность / userland:
- [ ] umask/права OnyxFS — канонический пункт passwd↔shadow↔ACL: не-root не может
      open /etc/shadow (sys_open); известный tradeoff — ring-2 passwd получает EPERM
      (самосмена сломана осознанно); полное решение: setuid-хелпер или per-user shadow
      (/etc/shadow/<user>) + правка ACL (create/rename ring≤1 only, handler/acl.rs:76,81).
- [ ] $5$-хэш несовместим с crypt(3) — документировать или перейти на стандартный формат.
- [ ] `passwd` с пустым текущим паролем — verify_old_password должен принимать пустой.

### Java runtime (большая цель, этапы):
- [ ] 1. Минимальный class loader (.class: constant pool, fields, methods)
      2. Интерпретатор байткода JVM (стек, локалы, базовые инструкции)
      3. Подмножество JDK (java/lang/Object, String, System, arraycopy…)
      4. GC (mark-sweep), исключения, потоки поверх proc/scheduler
      5. hello-world javac → /bin/jvm в QEMU/OC2R
      Формат: JVM-интерпретатор как onx-программа в ring1/2, без изменений ядра
      (нужен только mmap + файловый I/O — всё уже есть).

### Принятые компромиссы (не баги, не забыть почему):
- lto=false: fat/thin ломают линк ядра (__rust_alloc после LTO-merge); вернуться,
  когда global_allocator/panic-shims переживут LTO.
- KDF без memory-hardness — принятая позиция (10k SHA-256, onyx_core::crypto::kdf).
- Точечные clippy-исключения (34 allow) — обоснованы в коде (ABI-зеркала,
  per-bin init с датированными TODO); [workspace.lints] не вводились.
- Правило 2 остатки (fdt 13, onyxfs 12 файлов) — осознанно: папка = одна подсистема.
- onyx_init бины не собираются под host-тесты (сырой RISC-V asm) — тесты по крейтам
  (onyx_core/onyx_kernel), CI так и устроен.
