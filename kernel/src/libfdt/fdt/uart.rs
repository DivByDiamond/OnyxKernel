use super::FdtMmio;
use super::reader::{cstr_at, rd32, reg_base};
use super::walk::walk;

pub unsafe fn find_uart() -> Option<FdtMmio> {
    unsafe {
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
                    "reg" => base = reg_base(data),
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
        // QEMU-virt's device tree always carries an ns16550a node, so a missing
        // node means we are on OC2R/sedna: it ships a minimal DT with no
        // peripherals and the stock minux firmware hardcodes the console at
        // 0x10000448 (after two GoldfishRTCs and the virtio-console).
        if result.is_none() {
            return Some(FdtMmio {
                base: 0x1000_0448,
                irq: 10,
                reg_shift: 0,
            });
        }
        result
    }
}
