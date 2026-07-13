use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const MAX_HARTS: usize = 8;
pub const SEC_STACK_SIZE: usize = 4096;

#[unsafe(no_mangle)]
pub static mut G_SEC_STACKS: [u8; MAX_HARTS * SEC_STACK_SIZE] = [0; MAX_HARTS * SEC_STACK_SIZE];

// Bug (syscall SERIOUS #10): make G_ONLINE_HARTS atomic so concurrent
// secondary hart bring-up doesn't race on the increment.
static G_ONLINE_HARTS: AtomicU32 = AtomicU32::new(1);

#[unsafe(no_mangle)]
pub static mut G_RELEASE: u64 = 0;

#[unsafe(no_mangle)]
pub static mut G_KERNEL_ROOT_PA: u64 = 0;

pub fn current_hart() -> usize {
    let hartid: usize;
    unsafe {
        core::arch::asm!("mv {}, tp", out(reg) hartid);
    }
    hartid
}

static mut G_CPU_ONLINE: [bool; MAX_HARTS] =
    [true, false, false, false, false, false, false, false];

pub fn cpu_online(hart: usize) -> bool {
    unsafe { (*(&raw const G_CPU_ONLINE))[hart] }
}

pub unsafe fn set_cpu_online(hart: usize, v: bool) {
    (*(&raw mut G_CPU_ONLINE))[hart] = v;
}

pub struct SpinLock {
    locked: AtomicBool,
}

impl SpinLock {
    pub const fn new() -> Self {
        SpinLock {
            locked: AtomicBool::new(false),
        }
    }
    pub fn lock(&self) {
        while self.locked.swap(true, Ordering::Acquire) {
            while self.locked.load(Ordering::Relaxed) {
                spin_loop();
            }
        }
    }
    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

pub unsafe fn release_secondary_harts() {
    // Bug (syscall SERIOUS #9): use an atomic store with SeqCst so the
    // secondary harts (which spin on a read_volatile + wfi loop) are
    // guaranteed to observe the release on all cores. The previous
    // write_volatile was fine for a single secondary hart but had no
    // ordering guarantee for multi-hart systems.
    core::sync::atomic::AtomicU64::from_ptr(core::ptr::addr_of_mut!(G_RELEASE) as *mut u64)
        .store(1, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn secondary_entry() -> ! {
    let hartid: usize;
    core::arch::asm!("mv {0}, tp", out(reg) hartid);
    loop {
        if core::ptr::read_volatile(&raw const G_RELEASE) != 0 {
            break;
        }
        core::arch::asm!("wfi");
    }
    let sp = &raw const G_SEC_STACKS as *const u8 as usize + (hartid + 1) * SEC_STACK_SIZE;
    let entry = secondary_kmain as *const () as usize;
    let root_pa = core::ptr::read_volatile(&raw const G_KERNEL_ROOT_PA);
    let satp = if root_pa != 0 {
        (8u64 << 60) | (root_pa >> 12)
    } else {
        0
    };
    core::arch::asm!(
        "mv sp, {0}",
        "csrw mepc, {1}",
        "li t0, 1 << 11",
        "csrs mstatus, t0",
        "li t0, 1 << 12",
        "csrc mstatus, t0",
        "li t0, 1 << 7",
        "csrc mstatus, t0",
        "csrw satp, {2}",
        "sfence.vma zero, zero",
        "mret",
        in(reg) sp,
        in(reg) entry,
        in(reg) satp,
        options(noreturn),
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn secondary_kmain() -> ! {
    let hartid: usize;
    core::arch::asm!("mv {0}, tp", out(reg) hartid);
    crate::proc::process::set_cpu_online(hartid, true);
    // Bug (syscall SERIOUS #10): atomic fetch_add to avoid races between
    // concurrent secondary harts. The previous non-atomic increment could
    // lose updates when two harts booted simultaneously.
    G_ONLINE_HARTS.fetch_add(1, Ordering::SeqCst);
    crate::proc::scheduler::sched_enter_idle()
}

pub fn online_harts() -> u32 {
    G_ONLINE_HARTS.load(Ordering::SeqCst)
}
