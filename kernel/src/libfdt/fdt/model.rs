use super::reader::cstr_at;
use super::walk::walk;

pub unsafe fn model() -> &'static str {
    let mut found: Option<u32> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        for (name_off, _data) in props {
            if cstr_at(*name_off) == "model" {
                found = Some(*name_off);
                return true;
            }
        }
        false
    });
    match found {
        Some(_) => "from-fdt",
        None => "unknown",
    }
}
