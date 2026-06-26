use super::reader::{cstr_at, rd64};
use super::walk::walk;

pub unsafe fn find_plic() -> Option<u64> {
    let mut result: Option<u64> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        for (name_off, data) in props {
            if cstr_at(*name_off) == "reg" && data.len() >= 8 {
                let addr = rd64(data.as_ptr());
                if addr >= 0x0C00_0000 && addr < 0x0D00_0000 {
                    result = Some(addr);
                    return true;
                }
            }
        }
        false
    });
    result.or(Some(0x0C00_0000))
}
