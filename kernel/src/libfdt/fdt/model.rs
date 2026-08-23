use super::reader::cstr_at;
use super::walk::walk;

static mut G_MODEL: [u8; 64] = [0; 64];

fn copy_prop(data: &[u8]) {
    unsafe {
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

fn stored() -> &'static str {
    unsafe {
        let s = core::slice::from_raw_parts(&raw const G_MODEL as *const u8, 64);
        let len = s.iter().position(|&b| b == 0).unwrap_or(0);
        if len == 0 {
            return "";
        }
        core::str::from_utf8(&s[..len]).unwrap_or("")
    }
}

/// Read the root node's `model` property into a static buffer.
pub unsafe fn model() -> &'static str { unsafe {
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
}}

/// Detect the OC2R/sedna platform. The device tree carries the root
/// compatible "riscv-sedna" and/or the model "riscv-virtio,sedna" — both
/// contain "sedna", so scan the FDT memory for that substring. On this
/// platform the peripheral nodes (UART/PLIC/CLINT/virtio) are NOT present in
/// the device tree — the stock minux firmware uses hardcoded addresses, so
/// we do too.
pub unsafe fn is_sedna() -> bool { unsafe {
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
}}

/// Detect the QEMU virt machine. The device tree model is
/// "riscv-virtio,qemu" and the root compatible contains "qemu". Like
/// `is_sedna`, this scans the raw FDT memory for the substring. On QEMU
/// virt there is no USB host controller by default, so probing the
/// SG2000 EHCI/OHCI addresses (0x04C00000) would raise a load access
/// fault on unmapped MMIO.
pub unsafe fn is_qemu() -> bool { unsafe {
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
}}
