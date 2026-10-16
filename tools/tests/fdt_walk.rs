mod fdt {
    pub const FDT_BEGIN_NODE: u32 = 1;
    pub const FDT_END_NODE: u32 = 2;
    pub const FDT_PROP: u32 = 3;
    pub const FDT_NOP: u32 = 4;
    pub const FDT_END: u32 = 9;
    pub static mut G_STRUCT: usize = 0;
    pub static mut G_STRUCT_SIZE: usize = 0;
    pub static mut G_TOTALSIZE: usize = 0;
    pub static mut G_DTB: usize = 0;
    pub mod reader {
        pub unsafe fn rd32(p: *const u8) -> u32 {
            unsafe { u32::from_be(core::ptr::read_unaligned(p.cast())) }
        }
    }
    pub mod walk {
        include!("../../kernel/src/libfdt/fdt/walk.rs");
    }
}
#[test]
fn reports_parent_once_and_leaf_once() {
    fn word(v: &mut Vec<u8>, w: u32) {
        v.extend(w.to_be_bytes());
    }
    fn node(v: &mut Vec<u8>, name: &str) {
        word(v, 1);
        v.extend(name.as_bytes());
        v.push(0);
        while v.len() % 4 != 0 {
            v.push(0);
        }
    }
    fn prop(v: &mut Vec<u8>, value: u32) {
        word(v, 3);
        word(v, 4);
        word(v, 0);
        word(v, value);
    }
    let mut blob = vec![];
    node(&mut blob, "");
    prop(&mut blob, 11);
    node(&mut blob, "soc");
    prop(&mut blob, 22);
    node(&mut blob, "virtio@1000");
    prop(&mut blob, 33);
    word(&mut blob, 2);
    word(&mut blob, 2);
    word(&mut blob, 2);
    word(&mut blob, 9);
    let mut seen = vec![];
    unsafe {
        fdt::G_STRUCT = blob.as_ptr() as usize;
        fdt::G_STRUCT_SIZE = blob.len();
        fdt::walk::walk(&mut |name, props| {
            seen.push((name.to_owned(), props.len()));
            false
        });
    }
    assert_eq!(
        seen,
        vec![("".into(), 1), ("soc".into(), 1), ("virtio@1000".into(), 1)]
    );
}
