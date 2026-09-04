# Preloaded GDB session for live OnyxKernel debugging.
# Loaded by scripts/debug_qemu.sh, which connects the target first:
#   gdb -q -ex "file <kernel.elf>" -ex "target remote localhost:$GDBPORT" \
#       -ex "source scripts/debug.gdb"
#
# Breakpoints default to the cold suspects of the open SMP crash
# (OnyxKernel todo.md, "-smp 2" investigation). The hot paths stay
# commented out: they fire at every timer tick / trap and make single
# stepping impractical. Uncomment when the bug is narrowed further.

set pagination off
set confirm off
set print pretty on
set breakpoint pending on

# Boot path: first kernel instruction after OpenSBI hands over
break onyx_kernel::kmain

# Scheduler / lifecycle suspects (todo.md recommendation)
break onyx_kernel::proc::scheduler::sched::steal
break onyx_kernel::proc::scheduler::runqueue::dequeue
break onyx_kernel::proc::lifecycle::exit::exit
break onyx_kernel::proc::spawn::publish_ready
break onyx_kernel::proc::spawn::create_user
break onyx_kernel::proc::scheduler::idle::seed_boot_hart_idle_context

# Hot paths - uncomment when needed (fire at every timer tick / trap):
# break onyx_kernel::proc::scheduler::sched::sched_yield
# break onyx_kernel::srv::trap::handle

continue
