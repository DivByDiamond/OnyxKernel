use super::reader::{cstr_at, rd32, rd64};
use super::walk::walk;
use super::FdtMmio;

pub unsafe fn find_sdhci() -> Option<FdtMmio> {
    let mut result: Option<FdtMmio> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        let mut base = 0u64;
        let mut irq = 0u32;
        let mut is_sdhci = false;
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
                        if s == b"qemu,sdhci"
                            || s == b"generic-sdhci"
                            || s == b"arasan,sdhci"
                            || s == b"snps,dw-sdhci"
                            || s == b"sifive,sdio"
                        {
                            is_sdhci = true;
                        }
                        start += end + 1;
                    }
                }
                "reg" if data.len() >= 8 => base = rd64(data.as_ptr()),
                "interrupts" if data.len() >= 4 => irq = rd32(data.as_ptr()),
                _ => {}
            }
        }
        if is_sdhci && base != 0 {
            result = Some(FdtMmio {
                base,
                irq,
                reg_shift: 0,
            });
            return true;
        }
        false
    });
    result
}
