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
    unsafe {
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
}

pub unsafe fn unregister(name: &str) -> bool {
    unsafe {
        for slot in G_MODULES.iter_mut() {
            if matches!(slot, Some(m) if m.name == name) {
                *slot = None;
                return true;
            }
        }
        false
    }
}

pub fn find_by_name(name: &str) -> Option<&'static Module> {
    unsafe {
        for slot in G_MODULES.iter().flatten() {
            if slot.name == name {
                return Some(slot);
            }
        }
    }
    None
}

pub fn count() -> usize {
    unsafe { G_MODULES.iter().flatten().count() }
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
        for m in G_MODULES.iter().flatten() {
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
    pos
}

#[cfg(test)]
mod tests;
