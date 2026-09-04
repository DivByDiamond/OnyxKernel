use crate::arch::csr;
use crate::arch::regs::SSTATUS_SIE;
use crate::fs::vfs;
use crate::mm::heap;
use crate::proc;
use onyx_core::fmt::Arg;

/// # Safety
///
/// Terminal boot step: loads /bin/init, creates pid 1, enables S-mode
/// interrupts, releases secondary harts and enters user mode without
/// returning. Must run once on the boot hart with VFS and traps ready.
pub(crate) unsafe fn launch() -> ! {
    // SAFETY: one-shot boot call; proc state is initialized (proc::init) before create_user/enter_user,
    // and SIE is only enabled after the trap path is armed.
    unsafe {
        let path = b"/bin/init";
        let token = match vfs::open(path, vfs::PERM_READ | vfs::PERM_SEEK) {
            Ok(t) => t,
            Err(e) => {
                crate::kerr!("kmain", "open /bin/init failed: %s", Arg::from(e.as_str()));
                crate::srv::klog::halt();
            }
        };
        let mut size = 0u32;
        vfs::stat(token, &mut size).ok();
        crate::kinf!("kmain", "/bin/init size=%d", Arg::from(size));

        let img = match heap::kmalloc(size as usize) {
            Ok(p) => p,
            Err(e) => {
                crate::kerr!("kmain", "kmalloc failed: %s", Arg::from(e.as_str()));
                crate::srv::klog::halt();
            }
        };
        vfs::read(token, img, size).ok();
        vfs::close(token).ok();

        let r = match crate::proc::onx::load(img, size as usize) {
            Ok(r) => r,
            Err(e) => {
                crate::kerr!("kmain", "onx_load failed: %s", Arg::from(e.as_str()));
                crate::srv::klog::halt();
            }
        };
        heap::kfree(img);

        crate::kinf!(
            "onx",
            "entry=%p root=%p ustack=%p ring=%d",
            Arg::from(r.entry),
            Arg::from(r.root_pa),
            Arg::from(r.ustack),
            Arg::from(r.ring as u32)
        );

        proc::init();
        // Root-cause fix (SMP crash, todo.md "Отдельный SMP-краш под
        // -smp 2"): unlike every other hart, the boot hart never calls
        // `sched_enter_idle()` itself — it drops straight into
        // `enter_user(1)` below — so nothing would ever populate
        // `G_HART_IDLE_TF[0]` before the first time this hart genuinely
        // needs to idle (everything it ran migrated away via work-stealing,
        // or exited). Seed it now with a valid resume context instead of
        // leaving `sched_yield` to discover a zeroed frame later.
        proc::seed_boot_hart_idle_context(0);
        let ring = if r.ring == 1 {
            proc::PROC_RING_ROOT
        } else {
            proc::PROC_RING_USER
        };
        if let Err(e) = proc::create_user(
            r.entry,
            r.ustack,
            r.root_pa,
            proc::PROC_PID_INIT,
            0,
            r.heap_brk,
            ring,
            0,
            0,
            core::ptr::null_mut(),
        ) {
            crate::kerr!("kmain", "create_user failed: %s", Arg::from(e.as_str()));
            crate::srv::klog::halt();
        }

        csr::set_sstatus(SSTATUS_SIE);
        crate::kinf!(
            "proc",
            "entering user pid=1 entry=%p ring=%d",
            Arg::from(r.entry),
            Arg::from(ring as u32)
        );
        crate::arch::smp::release_secondary_harts();
        proc::enter_user(1);
    }
}
