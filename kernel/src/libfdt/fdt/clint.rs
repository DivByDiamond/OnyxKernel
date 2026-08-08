use super::reader::{cstr_at, reg_base};
use super::walk::walk;

pub unsafe fn find_clint() -> Option<u64> {
    let mut result: Option<u64> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        for (name_off, data) in props {
            if cstr_at(*name_off) == "reg" {
                let addr = reg_base(data);
                if addr >= 0x0200_0000 && addr < 0x0300_0000 {
                    result = Some(addr);
                    return true;
                }
            }
        }
        false
    });
    result.or(Some(0x0200_0000))
}
