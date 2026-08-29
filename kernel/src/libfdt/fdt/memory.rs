use super::FdtMemory;
use super::reader::{cstr_at, rd64, rd64_hi};
use super::walk::walk;

/// # Safety
///
/// `fdt::init()` must have succeeded; called once during single-threaded
/// boot (before secondary harts start).
pub unsafe fn memory() -> Option<FdtMemory> {
    unsafe {
        // SAFETY: caller contract: init() validated the DTB; walk only reads
        // the magic-checked struct block single-threaded.
        let mut result: Option<FdtMemory> = None;
        walk(&mut |name, props: &[(u32, &[u8])]| {
            if name.starts_with("memory") {
                for (name_off, data) in props {
                    if cstr_at(*name_off) == "reg" && data.len() >= 16 {
                        let base = rd64(data.as_ptr());
                        let size = (rd64(data.as_ptr().add(8)))
                            | ((rd64_hi(data.as_ptr().add(8)) as u64) << 32);
                        result = Some(FdtMemory { base, size });
                        return true;
                    }
                }
            }
            false
        });
        result.or(Some(FdtMemory {
            base: 0x8000_0000,
            size: 0x1000_0000,
        }))
    }
}
