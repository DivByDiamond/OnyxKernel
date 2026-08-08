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
        for i in 0..len {
            (*m)[i] = data[i];
        }
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
pub unsafe fn model() -> &'static str {
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
    if found {
        stored()
    } else {
        "unknown"
    }
}

/// Detect the OC2R/sedna platform (root compatible "riscv-sedna").
pub unsafe fn is_sedna() -> bool {
    let mut found = false;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        for (name_off, data) in props {
            let name = cstr_at(*name_off);
            if name == "compatible" {
                let mut start = 0;
                while start < data.len() {
                    let end = data[start..]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(data.len() - start);
                    let s = &data[start..start + end];
                    if s == b"riscv-sedna" {
                        found = true;
                        return true;
                    }
                    start += end + 1;
                }
            }
        }
        false
    });
    found
}
