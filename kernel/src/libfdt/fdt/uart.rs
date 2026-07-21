use super::reader::{cstr_at, rd32, rd64};
use super::walk::walk;
use super::FdtMmio;

pub unsafe fn find_uart() -> Option<FdtMmio> {
    let mut result: Option<FdtMmio> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        let mut base = 0u64;
        let mut reg_shift = 0u32;
        let mut is_uart = false;
        for (name_off, data) in props {
            match cstr_at(*name_off) {
                "compatible" => {
                    let mut start = 0;
                    while start < data.len() {
                        let end = data[start..]
                            .iter()
                            .position(|&b| b == 0)
                            .unwrap_or(data.len() - start);
                        let s = &data[start..start + end];
                        if s == b"ns16550a" || s == b"ns16550" {
                            is_uart = true;
                        }
                        start += end + 1;
                    }
                }
                "reg" if data.len() >= 8 => base = rd64(data.as_ptr()),
                "reg-shift" if data.len() >= 4 => reg_shift = rd32(data.as_ptr()),
                _ => {}
            }
        }
        if is_uart && base != 0 {
            result = Some(FdtMmio {
                base,
                irq: 10,
                reg_shift,
            });
            return true;
        }
        false
    });
    result.or(Some(FdtMmio {
        base: 0x1000_0000,
        irq: 10,
        reg_shift: 0,
    }))
}
