use super::FdtMmio;
use super::reader::{cstr_at, rd32, rd64};
use super::walk::walk;

pub unsafe fn find_virtio(out: &mut [FdtMmio], max: usize) -> usize {
    let mut count = 0;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        if count >= max {
            return true;
        }
        let mut is_virtio = false;
        let mut base = 0u64;
        let mut irq = 0u32;
        for (name_off, data) in props {
            match cstr_at(*name_off) {
                "compatible" => {
                    let mut start = 0;
                    while start < data.len() {
                        let end = data[start..]
                            .iter()
                            .position(|&b| b == 0)
                            .unwrap_or(data.len() - start);
                        if &data[start..start + end] == b"virtio,mmio" {
                            is_virtio = true;
                        }
                        start += end + 1;
                    }
                }
                "reg" if data.len() >= 8 => base = rd64(data.as_ptr()),
                "interrupts" if data.len() >= 4 => irq = rd32(data.as_ptr()),
                _ => {}
            }
        }
        if is_virtio && base != 0 {
            out[count] = FdtMmio {
                base,
                irq,
                reg_shift: 0,
            };
            count += 1;
        }
        false
    });
    count
}
