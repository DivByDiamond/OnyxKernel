<p align="center">
<pre class="not-prose" style="text-align:center;font-family:monospace;">
    ███████                                               
  ███▒▒▒▒▒███                                             
 ███     ▒▒███ ████████   █████ ████ █████ █████          
▒███      ▒███▒▒███▒▒███ ▒▒███ ▒███ ▒▒███ ▒▒███           
▒███      ▒███ ▒███ ▒███  ▒███ ▒███  ▒▒▒█████▒            
▒▒███     ███  ▒███ ▒███  ▒███ ▒███   ███▒▒▒███           
 ▒▒▒███████▒   ████ █████ ▒▒███████  █████ █████          
   ▒▒▒▒▒▒▒    ▒▒▒▒ ▒▒▒▒▒   ▒▒▒▒▒███ ▒▒▒▒▒ ▒▒▒▒▒           
                           ███ ▒███                       
                          ▒▒██████                        
                           ▒▒▒▒▒▒                         
 █████   ████                                        ████ 
▒▒███   ███▒                                        ▒▒███ 
 ▒███  ███     ██████  ████████  ████████    ██████  ▒███ 
 ▒███████     ███▒▒███▒▒███▒▒███▒▒███▒▒███  ███▒▒███ ▒███ 
 ▒███▒▒███   ▒███████  ▒███ ▒▒▒  ▒███ ▒███ ▒███████  ▒███ 
 ▒███ ▒▒███  ▒███▒▒▒   ▒███      ▒███ ▒███ ▒███▒▒▒   ▒███ 
 █████ ▒▒████▒▒██████  █████     ████ █████▒▒██████  █████
▒▒▒▒▒   ▒▒▒▒  ▒▒▒▒▒▒  ▒▒▒▒▒     ▒▒▒▒ ▒▒▒▒▒  ▒▒▒▒▒▒  ▒▒▒▒▒ 
                                                          
                                                          
                                                                                                                              
</pre>
</p>

<p align="center"><em>A RISC-V 64-bit operating system kernel written in Rust</em></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-RISC--V%2064--bit-green" alt="RISC-V 64">
  <img src="https://img.shields.io/badge/language-Rust%20%7E98%25-orange" alt="Rust ~98%">
  <img src="https://img.shields.io/badge/version-v0.5-blue" alt="v0.5">
  <img src="https://img.shields.io/badge/MMU-Sv39-yellow" alt="Sv39 MMU">
  <img src="https://img.shields.io/badge/license-GPL--3.0-red" alt="GPL-3.0">
</p>

----

OnyxKernel is a 64-bit RISC-V operating system kernel (~98% Rust, assembly via
`global_asm!`) featuring a three-ring privilege model, a custom filesystem with
journaling and snapshots, preemptive multitasking, and a complete userland with
authentication.

Part of the [OnyxOS](https://github.com/DivByDiamond/OnyxOS) ecosystem. Booted by
[OnyxBoot](https://github.com/DivByDiamond/OnyxBoot).

----

## Key Features

- **Written in Rust**: ~98% Rust, privileged assembly via `global_asm!` macros
- **RISC-V 64-bit (rv64gc)**: Sv39 MMU, three-level page tables
- **3-ring privilege model**: Kernel (ring 0), Root Space (ring 1), User Space (ring 2)
- **Dynamic processes**: heap-allocated linked list, no `PROC_MAX` limit
- **OnyxExec v2 binary format**: dynamic segments (up to 256), ring1 flag, compression flag
- **OnyxFS v2 filesystem**: timestamps (crtime/mtime/atime/ctime), indirect blocks, 40-byte dirents
- **Flashback snapshots**: `snapshot_create` / `rollback` / `list` with RLE compression + COW data blocks
- **Write-ahead journal**: crash recovery on mount
- **VFS with mount table**: OnyxFS, procfs, ipcfs
- **IPC channels**: `chan_create` / `connect` / `send` / `recv` / `close`; named channels via `/ipc/*` VFS
- **Syscall ABI**: 100 syscalls, POSIX-flavoured `open` with `O_CREAT/O_TRUNC/O_APPEND/O_NONBLOCK`, `fstat`, `waitpid`, `fork`, `execve`, `getdents64` (batched), `ioctl`, `mprotect`, `sigaction`/`sigprocmask`/`sigreturn`, `clock_gettime`, `isatty`, `getentropy`, `setuid`/`setgid`, `fsync`, `truncate` with length (full zero/shrink/extend support), `ftruncate`, `readlink`/`symlink` (real implementation, not stubs), `chmod`/`fchmod` (real mode-bit storage in inode), `chown`/`fchown` (real uid/gid storage), `umask`, `sched_setaffinity`/`sched_getaffinity`, `poll` (POLLIN/POLLOUT/POLLERR + timeout), `net_connect`/`net_send`/`net_recv`/`net_close` (TCP client), `net_resolve` (DNS A-record lookup), plus the original `spawn`/`wait`/`exec`/`sbrk`/`kill`/`sigmask`/`snapshot_*`/`create`/`mkdir`/IPC channel set.
- **Dynamic module registry**: built-in drivers register as modules; `/proc/modules` lists loaded modules; infrastructure for future dynamic loading
- **Driver unit tests**: 18 unit tests for UART and VirtIO drivers (constants, struct layouts, initial state)
- **Preemptive multitasking**: timer tick scheduling with `NEED_RESCHED` → `sched_yield`
- **Signal delivery**: `SYS_kill` with full `sigaction` / `sigprocmask` / `sigreturn` support for user-space handlers; KILL and STOP cannot be caught or blocked.
- **Blocking wait**: `Waiting` state + `sched_yield` for child process notification; `waitpid` with `WNOHANG`.
- **Generous user heap**: 64 MiB ceiling (was 64 KiB) with on-demand page mapping via `sbrk`/`brk`; 256 KiB user stack.
- **Proper ELF-style initial stack**: argc + argv + envp + auxv (AT_PAGESZ, AT_RANDOM, AT_ENTRY, AT_UID/GID, AT_NULL); compatible with musl/picolibc/glibc `_start`.
- **Demand-paged heap**: `sbrk(N)` no longer just bumps a pointer; new pages are mapped on demand via `pmm::alloc_zero` + `vmm::map_one_pub`.
- **Framebuffer console**: PSF1/PSF2 font support, `/font/default.psf` loaded at boot
- **Hardware drivers**: UART (NS16550A), VirtIO (block/net/rng/gpu/console/input), SDHCI, GMAC, USB (xHCI/EHCI/OHCI), framebuffer, PCI, PLIC
- **FDT parser**: device tree discovery (memory, PLIC, devices)
- **Per-process FD table**: 16 slots per process with capability tokens
- **Userland**: `/bin/login`, `/bin/osh` (shell), `/bin/passwd`, `/bin/useradd`, `/bin/userdel`
- **Authentication**: `/etc/passwd` + `/etc/shadow`; first boot seeds root with a hashed empty password (change it with `passwd`)
- **/proc filesystem**: `version`, `cpuinfo`, `meminfo`, `uptime`, `load`, `stat`, `modules`

----

## Architecture

### Privilege Rings

| Ring | Name | Mode | Description |
|------|------|------|-------------|
| 0 | Kernel | S-mode | Full access: memory, CSR, interrupts, drivers |
| 1 | Root Space | U-mode | PID 1 (`/bin/init`); all syscalls; can `spawn`, `create`, `snapshot` |
| 2 | User Space | U-mode | Restricted syscall ACL; cannot access `/service/*` |

- `dropring(target)`: one-way privilege drop (0→1, 0→2, 1→2). Privilege escalation is prevented.
- `/bin/login` verifies passwd/shadow; root retains ring 1, while non-root login drops to ring 2 before `execve`. Both rings run in hardware U-mode.
- The shipped `/bin/osh` is RING1-tagged, so the non-root path cannot launch it. Login does not yet install passwd uid/gid; a complete unprivileged login session remains unfinished.

### Memory Layout (Sv39)

```
0x0000_0000_0000_0000 ──────────────────────
                         (unused)
0x0000_0000_0001_0000  user_base
                         ┌─────────────────────┐
                         │  code/data segments  │
                         ├─────────────────────┤
0x0000_0000_0100_0000  heap_base
                         │  user heap          │  (~252 MB via sbrk)
                         ├─────────────────────┤
0x0000_0000_2000_0000  ustack_top
                         │  user stack         │  (16 pages, 64 KB)
0x0000_0000_4000_0000 ──────────────────────
                         (kernel identity mapping, first 3 GB)
```

### Boot Chain

1. **OnyxBoot** (M-mode): loads `kernel.elf` from VirtIO/SDHCI, jumps to entry
2. **boot.S** (`kernel/src/arch/asm/boot.rs`): clears BSS, sets up PMP, delegates exceptions to S-mode, `mret` → `kmain`
3. **kmain** (`kernel/src/srv/main.rs`): UART, FDT, PMM, VMM, heap, traps, PLIC, VirtIO, VFS mount, load `/bin/init`, `enter_user(1)`

### Process Lifecycle

```
alloc_proc() → Ready → Running → Exited
                  ↑         ↓
                  └── sched_yield()

Running ──wait()──→ Waiting ──child exits──→ Ready
Running ──kill()──→ Exited (SIGKILL)
```

### OnyxExec v2 Binary Format

```
[0..3]   MAGIC "ONX\0"
[4..7]   flags (bit 0 = RING1, bit 1 = COMPRESSED)
[8..11]  entry offset
[12..15] num_segments
[16..23] entry_vaddr
──────── seg[0..num_segments] (32 bytes each) ────────
  [0..7]   vaddr
  [8..11]  filesz
  [12..15] memsz
  [16..19] offset (in file)
  [20..23] flags (VMM_R, VMM_W, VMM_X)
  [24..27] compressed_size (0 = uncompressed)
──────── raw segment data ────────
```

### Directory Structure

```
/bin/        executables (init, login, osh, passwd, useradd, userdel, hello)
/service/    root services (started by init)
/etc/        configuration (passwd, shadow)
/proc/       system information (version, cpuinfo, meminfo, uptime, load, stat)
/font/       fonts (default.psf)
/users/      user home directories (/users/username/)
```

----

## Building

### Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust nightly | ≥ 1.85 | Kernel & userland compilation |
| `riscv64-elf-gcc` | - | OnyxBoot cross-compilation |
| `qemu-system-riscv64` | - | Testing (optional) |
| `parted`, `mkfs.fat`, `mcopy` | - | Boot disk image creation |
| `make` | - | OnyxBoot build |

### Install Rust target

```console
$ rustup target add riscv64gc-unknown-none-elf
```

### Build the kernel

```console
$ cargo build --release --target riscv64gc-unknown-none-elf
```

### Full build + QEMU (one command)

```console
$ ./scripts/run_qemu.sh
```

This script:
1. Builds **OnyxBoot** bootloader (`make -C ../OnyxBoot`)
2. Builds **OnyxKernel**, **init**, and **tools** (`elf2onx`, `mkimage`, `psfgen`)
3. Converts userland ELFs → `.onx` via `elf2onx`
4. Generates PSF1 font via `psfgen`
5. Creates `disk.img` (OnyxFS) from manifest via `mkimage`
6. Creates partitioned `boot.img` (FAT32 + OnyxFS)
7. Launches QEMU

### Override OnyxBoot path

```console
$ ONYXBOOT_DIR=/path/to/OnyxBoot ./scripts/run_qemu.sh
```

----

## Running in QEMU

```console
$ qemu-system-riscv64 \
    -M virt -m 256M -smp 2 \
    -bios /path/to/OnyxBoot/bootloader.bin \
    -drive file=build/boot.img,format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -nographic -no-reboot
```

Expected output:

```
OnyxBoot v0.4 [riscv-virtio,qemu]

OnyxBoot boot menu
--------------------
  0: VirtIO @ 0x0000000010008000
--------------------
Select device (0-0, or enter for auto): loading kernel.elf [########]
jumping to kernel
[kernel] OnyxKernel v0.3 (RISC-V 64-bit)
[kernel] FDT parsed, memory: 256 MB
[kernel] PMM: bitmap + slab allocator ready
[kernel] VMM: Sv39 page tables initialized
[kernel] VirtIO block device probed
[kernel] OnyxFS mounted on /
[kernel] Loading /bin/init (ring 1)
[init] OnyxOS init v0.3 (root space)
[init] Starting /bin/login

OnyxOS Login
login: root
Login OK (root, ring 1)
osh> _
```

----

## Project Structure

| Path | Description |
|------|-------------|
| `kernel/` | Kernel core: arch, proc, mm, fs, drivers, syscall, ipc |
| `core/` | Shared library: OnyxExec/OnyxFS format definitions, parser, errno |
| `init/` | Userland binaries: init, login, osh, passwd, useradd, userdel |
| `tools/` | Host tools: `elf2onx`, `mkimage`, `psfgen` |
| `scripts/` | Build & run scripts (`run_qemu.sh`) |
| `docs/` | Architecture documentation |

### Kernel Module Map

| Module | Path | Description |
|--------|------|-------------|
| `arch` | `kernel/src/arch/` | RISC-V boot, traps, CSR, SMP, register definitions |
| `proc` | `kernel/src/proc/` | Process management, scheduler, spawn, signals |
| `mm` | `kernel/src/mm/` | PMM (bitmap + slab), VMM (Sv39), kernel heap |
| `fs/onyxfs` | `kernel/src/fs/onyxfs/` | OnyxFS v2: read, write, mkdir, journal, snapshots |
| `fs/vfs` | `kernel/src/fs/vfs/` | Virtual filesystem: mount table, FD tokens, open/close/read/write |
| `fs/procfs` | `kernel/src/fs/procfs/` | /proc filesystem: version, cpuinfo, meminfo, uptime, load, stat |
| `fs/ipcfs` | `kernel/src/fs/ipcfs/` | /ipc/* VFS entries for named IPC channels |
| `drivers` | `kernel/src/drivers/` | UART, VirtIO block, PCI, PLIC, framebuffer, fb_term |
| `syscall` | `kernel/src/syscall/` | Syscall ABI, handler, ACL, dispatch |
| `ipc` | `kernel/src/ipc/` | IPC channels: create, connect, send, recv, close |
| `srv` | `kernel/src/srv/` | kmain, trap handler, timer, kernel log |
| `font` | `kernel/src/font/` | PSF1/PSF2 font parser and glyph renderer |
| `libfdt` | `kernel/src/libfdt/` | Flattened Device Tree parser |

----

## Ecosystem

| Repository | Role | Language |
|---|---|---|
| [OnyxOS](https://github.com/DivByDiamond/OnyxOS) | Ecosystem: build, disk image, docs | Bash / Python |
| [OnyxKernel](https://github.com/DivByDiamond/OnyxKernel) | RISC-V 64 kernel (Sv39, 3 rings) | Rust |
| [OnyxBoot](https://github.com/DivByDiamond/OnyxBoot) | Bootloader (M-mode, FDT) | C++20 |
| [OnyxShell](https://github.com/DivByDiamond/OnyxShell) | User shell `osh` | Rust |
| [OnyxCompiller](https://github.com/DivByDiamond/OnyxCompiller) | C99 compiler `onyxcc` + linker | C99 |
| [OnyxApps](https://github.com/DivByDiamond/OnyxApps) | Userland applications | C99 |
| [OnyxDesktop](https://github.com/DivByDiamond/OnyxDesktop) | Desktop environment `onyxdesk` | C99 |

----

## Roadmap

### ✅ Done in v0.3 - v0.5
- Driver unit tests (UART, VirtIO): 18 tests for constants, structs, initial state
- Dynamic module registry: `/proc/modules`, built-in driver registration, future-loading infrastructure
- Unicode table support in PSF1/PSF2 fonts
- IPC named channels via `/ipc/*` VFS
- FDT-driven hardware discovery (replace hardcoded addresses)
- PLIC IRQ dispatch (up to 64 IRQ handlers)
- Framebuffer driver with PSF1/PSF2 and scrolling
- SDHCI driver (for Milk-V Duo S)
- `elf2onx` v2 output (compressed_size + RLE compression)
- `mkimage` v2 (snapshot area + journal in image)
- Panic recovery / kdump (stack trace, process dump, QEMU reboot)
- Multi-core (SMP) support (up to 8 harts, per-CPU queues, load balancing, CPU affinity)
- Full userland: init, login, osh, passwd, useradd, userdel
- Authentication: /etc/passwd + /etc/shadow, first-boot root account seeding
- /proc filesystem: version, cpuinfo, meminfo, uptime, load, stat, modules
- Per-process FD tables (16 slots with capability tokens)
- Network stack (Ethernet/IP/UDP/TCP) with DHCP, DNS resolver and syscall interface
- USB HID input: device enumeration into the slot registry, HID report-descriptor parsing and interrupt-IN polling (`bus/usb/`, live QEMU smoke `OnyxOS/scripts/qemu-usb-smoke.sh` with `qemu-xhci` + `usb-kbd`/`usb-tablet`); stick sessions expose joydev-compatible `/dev/jsN`
- Write-ahead journal + crash recovery on mount
- Flashback snapshots with RLE compression + COW
- Dynamic processes (no PROC_MAX limit)
- Preemptive multitasking (100 Hz timer)
- Signal delivery with user-space handlers
- 100 syscalls total

### Still open
- **USB multi-device & mass storage**: enumeration, HID parsing and interrupt-IN input are live (see above); still missing: multi-slot xHCI, EHCI interrupt transfers, USB mass-storage class
- **Blocking socket `recv`**: userspace busy-polls today; the kernel has no per-socket wait queue
- **TCP segmentation**: 536 bytes per segment with no IP fragmentation; MSS/path-MTU handling is pending
- **No TLS**: `obrowse` stays HTTP-only
- **Dynamic kernel modules**: loading kernel ELF modules from userspace (`/proc/modules` lists only built-in drivers)
- **Journal durability**: flush/barrier and power-loss/partial-sector behaviour are unverified on OC2R/physical media

----

## 32-bit Support (rv32gc / Sv32)

OnyxKernel was originally written for RISC-V 64-bit (rv64gc, Sv39
paging). A cfg-gated 32-bit support layer has been added in
`kernel/src/arch/bits.rs` so the same source can be compiled for both
targets. The module provides:

- Pointer-sized types (`PteVal`, `Va`, `Pa`) that are `u64` on rv64
  and `u32` on rv32.
- SATP mode constants (`SATP_MODE_SV39` for 64-bit, `SATP_MODE_SV32`
  for 32-bit).
- PTE constants (V/R/W/X/U/G/A/D, PPN_SHIFT, PPN_MASK, FLAGS_MASK)
  with the correct width for each target.
- Page-table geometry (PTES_PER_TABLE = 512 on Sv39, 1024 on Sv32;
  PAGING_LEVELS = 3 on Sv39, 2 on Sv32).
- VA index functions (l0_idx, l1_idx, l2_idx) with the correct bit
  positions for each paging mode.
- USER_TOP (1 GiB on Sv39, 2 GiB on Sv32).

### What's NOT yet ported

The constants and types are in place, but the actual code paths still
use 64-bit assumptions. A full 32-bit port would require:

1. **boot.S**: a new boot assembly for rv32 (different register
   widths, different `li` encoding for large constants).
2. **VMM walker**: `mm/vmm/walk.rs` assumes 3 levels (L2/L1/L0).
   Sv32 has only 2 levels (L1/L0). The walker needs a cfg gate.
3. **SATP encoding**: `install_root()` in `mm/vmm/mod.rs` hardcodes
   `SATP_MODE_SV39 | (root_pa >> 12)`. On rv32 the mode is bit 31
   (not bits 60-63) and the PPN is 20 bits (not 44).
4. **CLINT mtimecmp**: on rv32 the CLINT mtimecmp registers are
   32-bit. `srv/timer.rs` reads/writes them as 64-bit via two 32-bit
   halves, which works, but the address calculation differs.
5. **TrapFrame**: `arch/trap_frame.rs` uses `u64` for all registers.
   On rv32 they're `u32`. The struct layout needs a cfg gate.
6. **OnyxExec format**: the `.onx` header uses `u64` for entry/seg
   offsets. A 32-bit variant would use `u32`.
7. **libc ABI**: `UserStat` in `syscall/fs_sys/open_close.rs` uses
   `u64`/`i64` fields matching the rv64 lp64d ABI. rv32 uses `u32`/
   `i32` (ilp32 ABI).

### How to build for 32-bit (when port is complete)

```bash
# Add the rv32gc target
rustup target add riscv32gc-unknown-none-elf

# Build
cargo build --release --target riscv32gc-unknown-none-elf

# Or set it as the default in .cargo/config.toml:
# [build]
# target = "riscv32gc-unknown-none-elf"
```

The QEMU command for 32-bit testing:
```bash
qemu-system-riscv32 -M virt -m 256M -bios bootloader.bin \
    -drive file=disk.img,format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -nographic -serial mon:stdio
```

### Migration path

The 32-bit port is being done incrementally. Code that needs to be
pointer-width-aware should use the types from `arch::bits` instead of
hardcoding `u64`. Once all the code paths are ported, the cfg gates
in `bits.rs` will produce a working 32-bit kernel with no further
changes needed.

----

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

----

## Русская версия

OnyxKernel: 64-битное RISC-V ядро ОС (~98% Rust, ассемблер через
`global_asm!`) с трёхуровневой моделью привилегий, собственной файловой системой
с журналированием и снапшотами, вытесняющей многозадачностью и полноценным
юзерлендом с аутентификацией.

Часть экосистемы [OnyxOS](https://github.com/DivByDiamond/OnyxOS). Загружается
[OnyxBoot](https://github.com/DivByDiamond/OnyxBoot).

----

## Ключевые особенности

- **Написано на Rust**: ~98% Rust, привилегированный ассемблер через макросы `global_asm!`
- **RISC-V 64-bit (rv64gc)**: MMU Sv39, трёхуровневые страничные таблицы
- **3 кольца привилегий**: Ядро (ring 0), Root Space (ring 1), User Space (ring 2)
- **Динамические процессы**: heap-allocated связный список, нет лимита `PROC_MAX`
- **Бинарный формат OnyxExec v2**: динамические сегменты (до 256), флаг ring1, флаг сжатия
- **Файловая система OnyxFS v2**: временные метки (crtime/mtime/atime/ctime), косвенные блоки, 40-байтовые dirents
- **Снапшоты Flashback**: `snapshot_create` / `rollback` / `list` с RLE-сжатием + COW блоки данных
- **Журнал предзаписи (WAL)**: восстановление после сбоев при монтировании
- **VFS с таблицей монтирования**: OnyxFS, procfs, ipcfs
- **IPC каналы**: `chan_create` / `connect` / `send` / `recv` / `close`; именованные каналы через `/ipc/*` VFS
- **Syscall ABI**: 100 системных вызовов, POSIX-вариант `open` с `O_CREAT/O_TRUNC/O_APPEND/O_NONBLOCK`, `fstat`, `waitpid`, `fork`, `execve`, `getdents64` (батчинг), `ioctl`, `mprotect`, `sigaction`/`sigprocmask`/`sigreturn`, `clock_gettime`, `isatty`, `getentropy`, `setuid`/`setgid`, `fsync`, `truncate` с длиной (полная поддержка обнуления/сжатия/расширения), `ftruncate`, `readlink`/`symlink` (реальная реализация, не заглушки), `chmod`/`fchmod` (реальное хранение бит режима в inode), `chown`/`fchown` (реальное хранение uid/gid), `umask`, `sched_setaffinity`/`sched_getaffinity`, `poll` (POLLIN/POLLOUT/POLLERR + таймаут), `net_connect`/`net_send`/`net_recv`/`net_close` (TCP-клиент), `net_resolve` (DNS-запрос A-записи), плюс исходный набор `spawn`/`wait`/`exec`/`sbrk`/`kill`/`sigmask`/`snapshot_*`/`create`/`mkdir`/IPC-каналы.
- **Реестр динамических модулей**: встроенные драйверы регистрируются как модули; `/proc/modules` показывает список загруженных модулей; инфраструктура для будущей динамической загрузки
- **Юнит-тесты драйверов**: 18 тестов для UART и VirtIO (константы, структуры, начальное состояние)
- **Вытесняющая многозадачность**: планирование по таймеру с `NEED_RESCHED` → `sched_yield`
- **Доставка сигналов**: `SYS_kill` с полной поддержкой `sigaction`/`sigprocmask`/`sigreturn` для user-space обработчиков; SIGKILL и SIGSTOP нельзя перехватить или заблокировать.
- **Блокирующий wait**: состояние `Waiting` + `sched_yield` для уведомления о завершении потомка; `waitpid` с `WNOHANG`.
- **Просторная куча пользователя**: потолок 64 МиБ (раньше 64 КиБ) с маппингом страниц по требованию через `sbrk`/`brk`; стек пользователя 256 КиБ.
- **Правильный стартовый стек в стиле ELF**: argc + argv + envp + auxv (AT_PAGESZ, AT_RANDOM, AT_ENTRY, AT_UID/GID, AT_NULL); совместим с `_start` из musl/picolibc/glibc.
- **Куча с маппингом по требованию**: `sbrk(N)` больше не только двигает указатель; новые страницы маппируются по требованию через `pmm::alloc_zero` + `vmm::map_one_pub`.
- **Framebuffer-консоль**: поддержка шрифтов PSF1/PSF2, загрузка `/font/default.psf` при старте
- **Драйверы оборудования**: UART (NS16550A), VirtIO (block/net/rng/gpu/console/input), SDHCI, GMAC, USB (xHCI/EHCI/OHCI), фреймбуфер, PCI, PLIC
- **Парсер FDT**: обнаружение устройств через дерево устройств (память, PLIC, устройства)
- **Пер-процессная таблица FD**: 16 слотов на процесс с capability-токенами
- **Юзерленд**: `/bin/login`, `/bin/osh` (shell), `/bin/passwd`, `/bin/useradd`, `/bin/userdel`
- **Аутентификация**: `/etc/passwd` + `/etc/shadow`; при первой загрузке создаётся root с хэшем пустого пароля (смените его через `passwd`)
- **Файловая система /proc**: `version`, `cpuinfo`, `meminfo`, `uptime`, `load`, `stat`, `modules`

----

## Архитектура

### Кольца привилегий

| Кольцо | Название | Режим | Описание |
|--------|----------|-------|----------|
| 0 | Ядро | S-mode | Полный доступ: память, CSR, прерывания, драйверы |
| 1 | Root Space | U-mode | PID 1 (`/bin/init`); все syscall; может `spawn`, `create`, `snapshot` |
| 2 | User Space | U-mode | Ограниченный ACL syscall; нет доступа к `/service/*` |

- `dropring(target)`: однонаправленное понижение привилегий (0→1, 0→2, 1→2). Повышение привилегий невозможно.
- `/bin/login` проверяет passwd/shadow: root сохраняет ring 1, остальные переходят в ring 2 перед `execve`. Оба кольца работают в аппаратном U-mode.
- Стандартный `/bin/osh` имеет флаг RING1, поэтому запуск из ring 2 отклоняется. Login пока не устанавливает uid/gid из passwd; непривилегированная сессия требует доработки.

### Раскладка памяти (Sv39)

```
0x0000_0000_0000_0000 ──────────────────────
                         (не используется)
0x0000_0000_0001_0000  user_base
                         ┌─────────────────────┐
                         │  сегменты кода/данных│
                         ├─────────────────────┤
0x0000_0000_0100_0000  heap_base
                         │  куча пользователя  │  (~252 МБ через sbrk)
                         ├─────────────────────┤
0x0000_0000_2000_0000  ustack_top
                         │  стек пользователя  │  (16 страниц, 64 КБ)
0x0000_0000_4000_0000 ──────────────────────
                         (identity-маппинг ядра, первые 3 ГБ)
```

### Цепочка загрузки

1. **OnyxBoot** (M-mode): загружает `kernel.elf` с VirtIO/SDHCI, передаёт управление в точку входа
2. **boot.S** (`kernel/src/arch/asm/boot.rs`): обнуляет BSS, настраивает PMP, делегирует исключения в S-mode, `mret` → `kmain`
3. **kmain** (`kernel/src/srv/main.rs`): UART, FDT, PMM, VMM, куча, traps, PLIC, VirtIO, монтирование VFS, загрузка `/bin/init`, `enter_user(1)`

### Жизненный цикл процесса

```
alloc_proc() → Ready → Running → Exited
                  ↑         ↓
                  └── sched_yield()

Running ──wait()──→ Waiting ──поток вышел──→ Ready
Running ──kill()──→ Exited (SIGKILL)
```

### Бинарный формат OnyxExec v2

```
[0..3]   MAGIC "ONX\0"
[4..7]   flags (бит 0 = RING1, бит 1 = COMPRESSED)
[8..11]  смещение точки входа
[12..15] num_segments
[16..23] entry_vaddr
──────── seg[0..num_segments] (по 32 байта) ────────
  [0..7]   vaddr
  [8..11]  filesz
  [12..15] memsz
  [16..19] offset (в файле)
  [20..23] flags (VMM_R, VMM_W, VMM_X)
  [24..27] compressed_size (0 = без сжатия)
──────── сырые данные сегментов ────────
```

### Структура директорий

```
/bin/        исполняемые файлы (init, login, osh, passwd, useradd, userdel, hello)
/service/    root-сервисы (запускаются init-ом)
/etc/        конфигурация (passwd, shadow)
/proc/       информация о системе (version, cpuinfo, meminfo, uptime, load, stat)
/font/       шрифты (default.psf)
/users/      домашние директории пользователей (/users/username/)
```

----

## Сборка

### Необходимые инструменты

| Инструмент | Версия | Назначение |
|------------|--------|------------|
| Rust nightly | ≥ 1.85 | Компиляция ядра и юзерленда |
| `riscv64-elf-gcc` | - | Кросс-компиляция OnyxBoot |
| `qemu-system-riscv64` | - | Тестирование (опционально) |
| `parted`, `mkfs.fat`, `mcopy` | - | Создание загрузочного образа диска |
| `make` | - | Сборка OnyxBoot |

### Установка Rust-таргета

```console
$ rustup target add riscv64gc-unknown-none-elf
```

### Сборка ядра

```console
$ cargo build --release --target riscv64gc-unknown-none-elf
```

### Полная сборка + QEMU (одной командой)

```console
$ ./scripts/run_qemu.sh
```

Этот скрипт:
1. Собирает загрузчик **OnyxBoot** (`make -C ../OnyxBoot`)
2. Собирает **OnyxKernel**, **init** и **инструменты** (`elf2onx`, `mkimage`, `psfgen`)
3. Конвертирует ELF-бинарники юзерленда → `.onx` через `elf2onx`
4. Генерирует PSF1-шрифт через `psfgen`
5. Создаёт `disk.img` (OnyxFS) из манифеста через `mkimage`
6. Создаёт разделённый `boot.img` (FAT32 + OnyxFS)
7. Запускает QEMU

### Переопределение пути к OnyxBoot

```console
$ ONYXBOOT_DIR=/путь/к/OnyxBoot ./scripts/run_qemu.sh
```

----

## Запуск в QEMU

```console
$ qemu-system-riscv64 \
    -M virt -m 256M -smp 2 \
    -bios /путь/к/OnyxBoot/bootloader.bin \
    -drive file=build/boot.img,format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -nographic -no-reboot
```

Ожидаемый вывод:

```
OnyxBoot v0.4 [riscv-virtio,qemu]

OnyxBoot boot menu
--------------------
  0: VirtIO @ 0x0000000010008000
--------------------
Select device (0-0, or enter for auto): loading kernel.elf [########]
jumping to kernel
[kernel] OnyxKernel v0.3 (RISC-V 64-bit)
[kernel] FDT parsed, memory: 256 MB
[kernel] PMM: bitmap + slab allocator ready
[kernel] VMM: Sv39 page tables initialized
[kernel] VirtIO block device probed
[kernel] OnyxFS mounted on /
[kernel] Loading /bin/init (ring 1)
[init] OnyxOS init v0.3 (root space)
[init] Starting /bin/login

OnyxOS Login
login: root
Login OK (root, ring 1)
osh> _
```

----

## Структура проекта

| Путь | Описание |
|------|----------|
| `kernel/` | Ядро: arch, proc, mm, fs, drivers, syscall, ipc |
| `core/` | Общая библиотека: форматы OnyxExec/OnyxFS, парсер, errno |
| `init/` | Бинарники юзерленда: init, login, osh, passwd, useradd, userdel |
| `tools/` | Инструменты хоста: `elf2onx`, `mkimage`, `psfgen` |
| `scripts/` | Скрипты сборки и запуска (`run_qemu.sh`) |
| `docs/` | Документация по архитектуре |

### Карта модулей ядра

| Модуль | Путь | Описание |
|--------|------|----------|
| `arch` | `kernel/src/arch/` | Загрузка RISC-V, трапы, CSR, SMP, определения регистров |
| `proc` | `kernel/src/proc/` | Управление процессами, планировщик, spawn, сигналы |
| `mm` | `kernel/src/mm/` | PMM (bitmap + slab), VMM (Sv39), куча ядра |
| `fs/onyxfs` | `kernel/src/fs/onyxfs/` | OnyxFS v2: чтение, запись, mkdir, журнал, снапшоты |
| `fs/vfs` | `kernel/src/fs/vfs/` | Виртуальная ФС: таблица монтирования, FD-токены, open/close/read/write |
| `fs/procfs` | `kernel/src/fs/procfs/` | ФС /proc: version, cpuinfo, meminfo, uptime, load, stat |
| `fs/ipcfs` | `kernel/src/fs/ipcfs/` | Записи /ipc/* VFS для именованных IPC-каналов |
| `drivers` | `kernel/src/drivers/` | UART, VirtIO block, PCI, PLIC, фреймбуфер, fb_term |
| `syscall` | `kernel/src/syscall/` | Syscall ABI, обработчик, ACL, диспетчеризация |
| `ipc` | `kernel/src/ipc/` | IPC каналы: create, connect, send, recv, close |
| `srv` | `kernel/src/srv/` | kmain, обработчик трапов, таймер, журнал ядра |
| `font` | `kernel/src/font/` | Парсер шрифтов PSF1/PSF2 и рендерер глифов |
| `libfdt` | `kernel/src/libfdt/` | Парсер Flattened Device Tree |

----

## Экосистема

| Repository | Role | Language |
|---|---|---|
| [OnyxOS](https://github.com/DivByDiamond/OnyxOS) | Ecosystem: build, disk image, docs | Bash / Python |
| [OnyxKernel](https://github.com/DivByDiamond/OnyxKernel) | RISC-V 64 kernel (Sv39, 3 rings) | Rust |
| [OnyxBoot](https://github.com/DivByDiamond/OnyxBoot) | Bootloader (M-mode, FDT) | C++20 |
| [OnyxShell](https://github.com/DivByDiamond/OnyxShell) | User shell `osh` | Rust |
| [OnyxCompiller](https://github.com/DivByDiamond/OnyxCompiller) | C99 compiler `onyxcc` + linker | C99 |
| [OnyxApps](https://github.com/DivByDiamond/OnyxApps) | Userland applications | C99 |
| [OnyxDesktop](https://github.com/DivByDiamond/OnyxDesktop) | Desktop environment `onyxdesk` | C99 |

----

## Дорожная карта

### ✅ Сделано в v0.3 - v0.5
- Юнит-тесты драйверов (UART, VirtIO): 18 тестов для констант, структур, начального состояния
- Реестр динамических модулей: `/proc/modules`, регистрация встроенных драйверов, инфраструктура будущей загрузки
- Поддержка Unicode-таблицы в шрифтах PSF1/PSF2
- Именованные IPC-каналы через `/ipc/*` VFS
- Обнаружение оборудования через FDT (замена вшитых адресов)
- Диспетчеризация прерываний PLIC (до 64 обработчиков)
- Драйвер фреймбуфера с PSF1/PSF2 и скроллингом
- Драйвер SDHCI (для Milk-V Duo S)
- `elf2onx` v2 (compressed_size + RLE-сжатие)
- `mkimage` v2 (снапшоты + журнал в образе)
- Panic recovery / kdump (stack trace, дамп процессов, QEMU reboot)
- SMP (до 8 ядер, per-CPU очереди, load balancing, CPU affinity)
- Полный юзерленд: init, login, osh, passwd, useradd, userdel
- Аутентификация: /etc/passwd + /etc/shadow, первый запуск с созданием root
- /proc: version, cpuinfo, meminfo, uptime, load, stat, modules
- Per-process FD таблицы (16 слотов, capability токены)
- Сетевой стек (Ethernet/IP/UDP/TCP) с DHCP, DNS-резолвером и syscall-интерфейсом
- USB HID-ввод: перечисление устройств в реестр слотов, разбор HID report-descriptor и interrupt-IN опрос (`bus/usb/`, live QEMU-smoke `OnyxOS/scripts/qemu-usb-smoke.sh` с `qemu-xhci` + `usb-kbd`/`usb-tablet`); stick-сессии открывают joydev-совместимые `/dev/jsN`
- Write-ahead журнал + восстановление при монтировании
- Flashback снапшоты с RLE + COW
- Динамические процессы (без PROC_MAX)
- Preemptive multitasking (100 Hz)
- Сигналы с user-space обработчиками
- 100 системных вызовов

### Осталось сделать
- **USB multi-device и mass storage**: перечисление, HID-разбор и interrupt-IN ввод работают (см. выше); осталось: multi-slot xHCI, EHCI interrupt-передачи, класс mass-storage
- **Блокирующий `recv` сокета**: userspace сегодня опрашивает сокет сам; в ядре нет wait-очереди на сокет
- **TCP-сегментация**: 536 байт на сегмент без IP-фрагментации; обработка MSS/path-MTU не сделана
- **Нет TLS**: `obrowse` остаётся HTTP-only
- **Динамические модули ядра**: загрузка ELF-модулей ядра из userspace (`/proc/modules` показывает только встроенные драйверы)
- **Долговечность журнала**: flush/barrier и поведение при потере питания/частичных секторах не проверены на OC2R/физическом носителе

----

## Поддержка 32-bit (rv32gc / Sv32)

OnyxKernel изначально написан под RISC-V 64-bit (rv64gc, страничная
организация Sv39). В `kernel/src/arch/bits.rs` добавлен cfg-гейтированный слой
поддержки 32-bit, чтобы один и тот же исходник компилировался под обе цели.
Модуль предоставляет:

- Типы размером с указатель (`PteVal`, `Va`, `Pa`): `u64` на rv64
  и `u32` на rv32.
- Константы режима SATP (`SATP_MODE_SV39` для 64-bit, `SATP_MODE_SV32`
  для 32-bit).
- Константы PTE (V/R/W/X/U/G/A/D, PPN_SHIFT, PPN_MASK, FLAGS_MASK)
  корректной ширины для каждой цели.
- Геометрию таблиц страниц (PTES_PER_TABLE = 512 на Sv39, 1024 на Sv32;
  PAGING_LEVELS = 3 на Sv39, 2 на Sv32).
- Функции индексов VA (l0_idx, l1_idx, l2_idx) с корректными битовыми
  позициями для каждого режима пейджинга.
- USER_TOP (1 GiB на Sv39, 2 GiB на Sv32).

### Что ещё НЕ портировано

Константы и типы готовы, но сами пути кода всё ещё используют 64-битные
предположения. Полный 32-битный порт потребует:

1. **boot.S**: новый загрузочный ассемблер под rv32 (другая ширина
   регистров, другая кодировка `li` для больших констант).
2. **VMM walker**: `mm/vmm/walk.rs` рассчитан на 3 уровня (L2/L1/L0).
   В Sv32 только 2 уровня (L1/L0); обходчику нужен cfg-гейт.
3. **Кодирование SATP**: `install_root()` в `mm/vmm/mod.rs` жёстко задаёт
   `SATP_MODE_SV39 | (root_pa >> 12)`. На rv32 режим это бит 31
   (а не биты 60-63), а PPN занимает 20 бит (а не 44).
4. **CLINT mtimecmp**: на rv32 регистры CLINT mtimecmp 32-битные.
   `srv/timer.rs` читает/пишет их как 64-битные через две 32-битные
   половины, что работает, но расчёт адреса отличается.
5. **TrapFrame**: `arch/trap_frame.rs` использует `u64` для всех
   регистров. На rv32 они `u32`; layout структуры нужен cfg-гейт.
6. **Формат OnyxExec**: заголовок `.onx` использует `u64` для смещений
   entry/seg. 32-битный вариант использовал бы `u32`.
7. **libc ABI**: `UserStat` в `syscall/fs_sys/open_close.rs` использует
   поля `u64`/`i64` под ABI rv64 lp64d. rv32 использует `u32`/`i32`
   (ABI ilp32).

### Как собрать 32-битную версию (когда порт завершён)

```bash
# Добавить таргет rv32gc
rustup target add riscv32gc-unknown-none-elf

# Сборка
cargo build --release --target riscv32gc-unknown-none-elf

# Или сделать его стандартным в .cargo/config.toml:
# [build]
# target = "riscv32gc-unknown-none-elf"
```

Команда QEMU для 32-битного тестирования:
```bash
qemu-system-riscv32 -M virt -m 256M -bios bootloader.bin \
    -drive file=disk.img,format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -nographic -serial mon:stdio
```

### Путь миграции

32-битный порт делается инкрементально. Коду, чувствительному к ширине
указателя, следует использовать типы из `arch::bits` вместо жёсткого
кодирования `u64`. Когда все пути кода портированы, cfg-гейты в `bits.rs`
дадут рабочее 32-битное ядро без дальнейших изменений.

----

## Лицензия

GPL-3.0-or-later. См. [LICENSE](LICENSE).
