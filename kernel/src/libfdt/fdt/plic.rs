use super::reader::{cstr_at, reg_base};
use super::walk::walk;

pub unsafe fn find_plic() -> Option<u64> {
    unsafe {
        let mut result: Option<u64> = None;
        walk(&mut |_name, props: &[(u32, &[u8])]| {
            for (name_off, data) in props {
                if cstr_at(*name_off) == "reg" {
                    let addr = reg_base(data);
                    if (0x0C00_0000..0x0D00_0000).contains(&addr) {
                        result = Some(addr);
                        return true;
                    }
                }
            }
            false
        });
        result.or(Some(0x0C00_0000))
    }
}
