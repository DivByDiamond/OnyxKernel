use super::reader::cstr_at;
use super::walk::walk;

static mut G_MODEL: [u8; 64] = [0; 64];

/// # Safety
///
/// Writes the boot-time global `G_MODEL`; must run only during the
/// single-threaded FDT parse (before secondary harts start). `data[..len]`
/// is bounded by `min(63, data.len())`.
fn copy_prop(data: &[u8]) {
    unsafe {
        // SAFETY: static mut accessed only from the single-threaded boot
        // parse; len is clamped to 63 so both slices are in bounds.
        let m = &raw mut G_MODEL;
        (*m).fill(0);
        let len = data
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(data.len())
            .min(63);
        (&mut *m)[..len].copy_from_slice(&data[..len]);
    }
}

/// # Safety
///
/// Reads `G_MODEL`, which is only ever written by `copy_prop` during the
/// single-threaded boot parse; the 64-byte slice stays inside the static.
fn stored() -> &'static str {
    unsafe {
        // SAFETY: `&raw const G_MODEL` yields a valid 64-byte static;
        // UTF-8 validity is checked by from_utf8.
        let s = core::slice::from_raw_parts(&raw const G_MODEL as *const u8, 64);
        let len = s.iter().position(|&b| b == 0).unwrap_or(0);
        if len == 0 {
            return "";
        }
        core::str::from_utf8(&s[..len]).unwrap_or("")
    }
}

/// Read the root node's `model` property into a static buffer.
///
/// # Safety
///
/// `fdt::init()` must have succeeded; must run once during single-threaded
/// boot (the walk reads global DTB state, `G_MODEL` is written here).
pub unsafe fn model() -> &'static str {
    unsafe {
        // SAFETY: caller contract: init() ran (validated DTB) and this runs
        // single-threaded during boot; walk + copy_prop honor that.
        let mut found = false;
        walk(&mut |_name, props: &[(u32, &[u8])]| {
            for (name_off, data) in props {
                if cstr_at(*name_off) == "model" && !data.is_empty() {
                    copy_prop(data);
                    found = true;
                    return true;
                }
            }
            false
        });
        if found { stored() } else { "unknown" }
    }
}

/// Detect the OC2R/sedna platform. The device tree carries the root
/// compatible "riscv-sedna" and/or the model "riscv-virtio,sedna" — both
/// contain "sedna", so scan the FDT memory for that substring. On this
/// platform the peripheral nodes (UART/PLIC/CLINT/virtio) are NOT present in
/// the device tree — the stock minux firmware uses hardcoded addresses, so
/// we do too.
pub unsafe fn is_sedna() -> bool {
    unsafe {
        // SAFETY: G_DTB was set by init() (magic-validated, in mapped RAM);
        // the scan stays within 256 KiB of the DTB base. Caller contract:
        // the firmware places the DTB far enough from the RAM top that this
        // window is readable.
        let dtb = super::G_DTB;
        if dtb == 0 {
            return false;
        }
        let needle = b"sedna";
        for i in 0..0x40000usize {
            let p = (dtb + i) as *const u8;
            let mut ok = true;
            for (j, &nb) in needle.iter().enumerate() {
                if *p.add(j) != nb {
                    ok = false;
                    break;
                }
            }
            if ok {
                return true;
            }
        }
        false
    }
}

/// Detect the QEMU virt machine. The device tree model is
/// "riscv-virtio,qemu" and the root compatible contains "qemu". Like
/// `is_sedna`, this scans the raw FDT memory for the substring. On QEMU
/// virt there is no USB host controller by default, so probing the
/// SG2000 EHCI/OHCI addresses (0x04C00000) would raise a load access
/// fault on unmapped MMIO.
pub unsafe fn is_qemu() -> bool {
    unsafe {
        // SAFETY: G_DTB was set by init() (magic-validated, in mapped RAM);
        // the scan stays within 256 KiB of the DTB base. Caller contract:
        // the firmware places the DTB far enough from the RAM top that this
        // window is readable.
        let dtb = super::G_DTB;
        if dtb == 0 {
            return false;
        }
        let needle = b"qemu";
        for i in 0..0x40000usize {
            let p = (dtb + i) as *const u8;
            let mut ok = true;
            for (j, &nb) in needle.iter().enumerate() {
                if *p.add(j) != nb {
                    ok = false;
                    break;
                }
            }
            if ok {
                return true;
            }
        }
        false
    }
}
