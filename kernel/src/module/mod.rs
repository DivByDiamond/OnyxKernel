use core::fmt::Write;
use core::ptr;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModuleType {
    Driver,
    FileSystem,
    Misc,
}

impl ModuleType {
    pub fn as_str(self) -> &'static str {
        match self {
            ModuleType::Driver => "driver",
            ModuleType::FileSystem => "fs",
            ModuleType::Misc => "misc",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Module {
    pub name: &'static str,
    pub type_: ModuleType,
    pub init: bool,
}

pub const MAX_MODULES: usize = 64;

static mut G_MODULES: [Option<Module>; MAX_MODULES] = [None; MAX_MODULES];
static mut G_NMODULES: usize = 0;

pub unsafe fn register(name: &'static str, type_: ModuleType) {
    if G_NMODULES >= MAX_MODULES {
        return;
    }
    let idx = G_NMODULES;
    G_MODULES[idx] = Some(Module {
        name,
        type_,
        init: true,
    });
    G_NMODULES += 1;
}

pub unsafe fn unregister(name: &str) -> bool {
    for i in 0..MAX_MODULES {
        if let Some(ref m) = G_MODULES[i] {
            if m.name == name {
                G_MODULES[i] = None;
                return true;
            }
        }
    }
    false
}

pub fn find_by_name(name: &str) -> Option<&'static Module> {
    unsafe {
        for i in 0..MAX_MODULES {
            if let Some(ref m) = G_MODULES[i] {
                if m.name == name {
                    return Some(m);
                }
            }
        }
    }
    None
}

pub fn count() -> usize {
    unsafe {
        let mut cnt = 0usize;
        for i in 0..MAX_MODULES {
            if G_MODULES[i].is_some() {
                cnt += 1;
            }
        }
        cnt
    }
}

fn write_bytes(buf: &mut [u8], pos: &mut usize, bytes: &[u8]) {
    for &b in bytes {
        if *pos < buf.len() {
            buf[*pos] = b;
        }
        *pos += 1;
    }
}

fn write_spaces(buf: &mut [u8], pos: &mut usize, n: usize) {
    for _ in 0..n {
        if *pos < buf.len() {
            buf[*pos] = b' ';
        }
        *pos += 1;
    }
}

pub fn format_list(buf: &mut [u8]) -> usize {
    let mut pos = 0usize;
    unsafe {
        for i in 0..MAX_MODULES {
            if let Some(ref m) = G_MODULES[i] {
                let init_s: &[u8] = if m.init { b"Live" } else { b"Builtin" };
                write_spaces(buf, &mut pos, 3);
                write_bytes(buf, &mut pos, m.name.as_bytes());
                write_spaces(buf, &mut pos, 2);
                write_bytes(buf, &mut pos, m.type_.as_str().as_bytes());
                write_spaces(buf, &mut pos, 4);
                write_bytes(buf, &mut pos, init_s);
                write_bytes(buf, &mut pos, b"\n");
            }
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_type_as_str() {
        assert_eq!(ModuleType::Driver.as_str(), "driver");
        assert_eq!(ModuleType::FileSystem.as_str(), "fs");
        assert_eq!(ModuleType::Misc.as_str(), "misc");
    }

    #[test]
    fn test_register_and_count() {
        unsafe {
            let before = count();
            register("test_mod", ModuleType::Misc);
            assert_eq!(count(), before + 1);
            unregister("test_mod");
            assert_eq!(count(), before);
        }
    }

    #[test]
    fn test_find_by_name() {
        unsafe {
            register("find_me", ModuleType::Driver);
            let m = find_by_name("find_me");
            assert!(m.is_some());
            assert_eq!(m.unwrap().name, "find_me");
            assert_eq!(m.unwrap().type_, ModuleType::Driver);
            unregister("find_me");
        }
    }

    #[test]
    fn test_unregister_nonexistent() {
        unsafe {
            assert!(!unregister("does_not_exist"));
        }
    }

    #[test]
    fn test_format_list() {
        unsafe {
            register("fmt_test", ModuleType::FileSystem);
            let mut buf = [0u8; 256];
            let len = format_list(&mut buf);
            let s = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(s.contains("fmt_test"));
            assert!(s.contains("fs"));
            unregister("fmt_test");
        }
    }

    #[test]
    fn test_module_type_equality() {
        assert_eq!(ModuleType::Driver, ModuleType::Driver);
        assert_ne!(ModuleType::Driver, ModuleType::FileSystem);
    }

    #[test]
    fn test_module_not_zero_sized() {
        assert!(core::mem::size_of::<Module>() > 0);
    }
}
