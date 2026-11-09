use crate::syscalls;
use core::arch::asm;

pub fn backoff_sleep(fails: u32) {
    let exp = fails.min(6);
    let delay_us: u64 = (1u64 << exp) * 250_000;
    let ts: [u64; 2] = [delay_us / 1_000_000, (delay_us % 1_000_000) * 1000];
    unsafe {
        syscalls::nanosleep(ts.as_ptr(), core::ptr::null_mut());
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
