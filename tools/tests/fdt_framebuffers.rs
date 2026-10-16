//! Runs the real FDT walker/reader/display parser against nested display nodes.
mod fdt {
    pub const FDT_BEGIN_NODE: u32 = 1; pub const FDT_END_NODE: u32 = 2;
    pub const FDT_PROP: u32 = 3; pub const FDT_NOP: u32 = 4; pub const FDT_END: u32 = 9;
    pub static mut G_STRUCT: usize = 0; pub static mut G_STRUCT_SIZE: usize = 0;
    pub static mut G_TOTALSIZE: usize = 0; pub static mut G_DTB: usize = 0;
    pub static mut G_STRINGS: usize = 0; pub static mut G_STRINGS_SIZE: usize = 0;
    pub mod reader { include!("../../kernel/src/libfdt/fdt/reader.rs"); }
    pub mod walk { include!("../../kernel/src/libfdt/fdt/walk.rs"); }
    pub use walk::walk;
}

pub use fdt::{reader, walk};
#[path = "../../kernel/src/libfdt/fdt/framebuffer.rs"]
mod framebuffer;

#[derive(Default)]
struct Tree { data: Vec<u8>, strings: Vec<u8> }
impl Tree {
    fn word(&mut self, value: u32) { self.data.extend(value.to_be_bytes()); }
    fn begin(&mut self, name: &str) { self.word(1); self.data.extend(name.as_bytes()); self.data.push(0); self.pad(); }
    fn pad(&mut self) { while self.data.len() % 4 != 0 { self.data.push(0); } }
    fn prop(&mut self, name: &str, value: &[u8]) {
        let offset = self.strings.len() as u32; self.strings.extend(name.as_bytes()); self.strings.push(0);
        self.word(3); self.word(value.len() as u32); self.word(offset); self.data.extend(value); self.pad();
    }
    fn display(&mut self, name: &str, address: u64, width: u32, height: u32, stride: u32, status: &[u8], format: &[u8]) {
        self.begin(name); self.prop("compatible", b"simple-framebuffer\0");
        let mut reg = address.to_be_bytes().to_vec(); reg.extend((height as u64 * stride as u64).to_be_bytes());
        self.prop("reg", &reg); self.prop("width", &width.to_be_bytes()); self.prop("height", &height.to_be_bytes());
        self.prop("stride", &stride.to_be_bytes()); self.prop("format", format); self.prop("status", status); self.word(2);
    }
}

#[test]
fn discovers_multiple_surfaces_without_ancestor_duplicates_and_rejects_invalid_nodes() {
    let mut t = Tree::default(); t.begin(""); t.begin("soc");
    t.display("framebuffer@30000000", 0x30000000, 320, 200, 640, b"okay\0", b"r5g6b5\0");
    t.display("framebuffer@30100000", 0x30100000, 640, 480, 1280, b"okay\0", b"r5g6b5\0");
    t.display("disabled", 0x30200000, 640, 480, 1280, b"disabled\0", b"r5g6b5\0");
    t.display("bad-format", 0x30300000, 640, 480, 1280, b"okay\0", b"unsupported\0");
    t.display("short-stride", 0x30400000, 640, 480, 12, b"okay\0", b"r5g6b5\0");
    t.word(2); t.begin("chosen");
    // Legacy firmware may mirror the first physical surface under /chosen.
    t.display("legacy-copy", 0x30000000, 320, 200, 640, b"okay\0", b"r5g6b5\0");
    t.display("framebuffer@30500000", 0x30500000, 160, 100, 640, b"okay\0", b"x8r8g8b8\0");
    t.word(2); t.word(2); t.word(9);
    unsafe {
        fdt::G_STRUCT = t.data.as_ptr() as usize; fdt::G_STRUCT_SIZE = t.data.len();
        fdt::G_STRINGS = t.strings.as_ptr() as usize; fdt::G_STRINGS_SIZE = t.strings.len();
        let mut displays = [framebuffer::SimpleFramebuffer::default(); 8];
        assert_eq!(framebuffer::find_simple_framebuffers(&mut displays), 3);
        assert_eq!((displays[0].pa, displays[0].width, displays[0].height, displays[0].bpp), (0x30000000,320,200,16));
        assert_eq!((displays[1].pa, displays[1].width, displays[1].height, displays[1].bpp), (0x30100000,640,480,16));
        assert_eq!((displays[2].pa, displays[2].bpp), (0x30500000,32));
        assert_eq!(framebuffer::find_simple_framebuffer().unwrap().pa, 0x30000000);
        assert_eq!(framebuffer::find_simple_framebuffers(&mut displays[..1]), 1);
    }
}
