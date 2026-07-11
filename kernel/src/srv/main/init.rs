use crate::arch::csr;
use crate::arch::regs::SSTATUS_SIE;
use crate::fs::vfs;
use crate::mm::heap;
use crate::proc;
use onyx_core::fmt::Arg;

pub(crate) unsafe fn launch() -> ! {
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
