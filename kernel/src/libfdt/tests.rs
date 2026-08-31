//! Host unit-tests for the libfdt bounds-checking fixes (todo P1 #7).
//!
//! All assertions live in ONE combined #[test] fn because the FDT globals
//! (G_DTB / G_STRUCT / G_STRINGS / sizes) are process-global statics and the
//! host harness runs #[test] fns in parallel.
//!
//! IMPORTANT: rejected-blob cases must go through `fdt::init_from` directly
//! — the public `fdt::init` falls back to `scan_fdt_in_ram`, whose fixed
//! 0x8000_0000..0x9000_0000 RAM window is only mapped on the target boards,
//! not under the host test process.

/// FDT header field offsets (spec: 40-byte header).
const OFF_TOTALSIZE: usize = 4;
const OFF_STRUCT: usize = 8;
const OFF_STRINGS: usize = 12;
const OFF_STRINGS_SIZE: usize = 0x20;
const OFF_STRUCT_SIZE: usize = 0x24;

const FDT_MAGIC: u32 = 0xD00D_FEED;
const TOK_BEGIN_NODE: u32 = 1;
const TOK_END_NODE: u32 = 2;
const TOK_PROP: u32 = 3;
const TOK_END: u32 = 9;

fn put32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_be_bytes());
}

fn get32(buf: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

/// Build a minimal VALID dtb: header + one root node with `compatible`
/// ("testmodel,qemu-ish") and `model` ("test model") properties.
fn valid_blob() -> [u8; 512] {
    let mut b = [0u8; 512];
    let struct_off = 0x40usize;
    let mut p = struct_off;

    // begin root node (root name is a single NUL, padded to 4)
    put32(&mut b, p, TOK_BEGIN_NODE);
    p += 4;
    b[p] = 0;
    p += 4;

    // prop "compatible" (name off 0) = "testmodel,qemu-ish\0"
    let compatible_data = b"testmodel,qemu-ish\0";
    put32(&mut b, p, TOK_PROP);
    p += 4;
    put32(&mut b, p, compatible_data.len() as u32);
    p += 4;
    put32(&mut b, p, 0);
    p += 4;
    b[p..p + compatible_data.len()].copy_from_slice(compatible_data);
    p += compatible_data.len();
    p = (p + 3) & !3;

    // prop "model" (name off 11) = "test model\0"
    let model_data = b"test model\0";
    put32(&mut b, p, TOK_PROP);
    p += 4;
    put32(&mut b, p, model_data.len() as u32);
    p += 4;
    put32(&mut b, p, 11);
    p += 4;
    b[p..p + model_data.len()].copy_from_slice(model_data);
    p += model_data.len();
    p = (p + 3) & !3;

    put32(&mut b, p, TOK_END_NODE);
    p += 4;
    put32(&mut b, p, TOK_END);
    p += 4;
    let struct_size = p - struct_off;

    let strings_off = p;
    let strings = b"compatible\0model\0";
    b[strings_off..strings_off + strings.len()].copy_from_slice(strings);
    let strings_size = strings.len();
    let totalsize = strings_off + strings_size;

    put32(&mut b, 0, FDT_MAGIC);
    put32(&mut b, OFF_TOTALSIZE, totalsize as u32);
    put32(&mut b, OFF_STRUCT, struct_off as u32);
    put32(&mut b, OFF_STRINGS, strings_off as u32);
    put32(&mut b, OFF_STRINGS_SIZE, strings_size as u32);
    put32(&mut b, OFF_STRUCT_SIZE, struct_size as u32);
    b
}

#[test]
fn test_fdt_bounds_combined() {
    unsafe {
        // ── 1) Valid blob: init succeeds, walk finds the root node,
        //    prop_name resolves, model + platform scan work.
        let blob = valid_blob();
        let totalsize = get32(&blob, OFF_TOTALSIZE);
        assert!(crate::libfdt::fdt::init(blob.as_ptr() as usize));

        let mut begin_count = 0usize;
        let mut saw_compatible = false;
        crate::libfdt::fdt::walk(&mut |name, props| {
            if name.is_empty() {
                begin_count += 1;
                for (name_off, data) in props {
                    if crate::libfdt::fdt::prop_name(*name_off) == "compatible" {
                        saw_compatible = true;
                        assert!(
                            core::str::from_utf8(data)
                                .unwrap()
                                .starts_with("testmodel,qemu-ish")
                        );
                    }
                }
                return false;
            }
            false
        });
        assert_eq!(begin_count, 1);
        assert!(saw_compatible);

        // Platform scan must stay inside the (tiny) totalsize yet still
        // find "qemu" in the compatible string; "sedna" is absent.
        assert!(crate::libfdt::fdt::is_qemu());
        assert!(!crate::libfdt::fdt::is_sedna());
        assert_eq!(crate::libfdt::fdt::model(), "test model");

        // ── 2) prop_name bounds: offsets at/past the strings-block size
        //    return "" instead of scanning arbitrary memory.
        assert_eq!(crate::libfdt::fdt::prop_name(u32::MAX), "");
        assert_eq!(crate::libfdt::fdt::prop_name(0xFFFF), "");

        // ── 3) FDT_PROP whose payload length lies past the struct-block
        //    end: the walk must stop instead of reading past `end`.
        let mut trunc = valid_blob();
        // Blob layout: struct_off=0x40 → [0x40]=BEGIN_NODE,[0x44]=name NUL
        // pad,[0x48]=TOK_PROP,[0x4C]=prop-len field. Claim 0x300 payload
        // bytes where only ~20 fit before the struct-block end. The header
        // itself stays self-consistent (init_from accepts it) — the
        // corruption lives in the CONTENT, which only walk() can catch.
        put32(&mut trunc, 0x4C, 0x300);
        assert!(crate::libfdt::fdt::init(trunc.as_ptr() as usize));
        let mut visits = 0usize;
        crate::libfdt::fdt::walk(&mut |_name, _props| {
            visits += 1;
            false
        });
        // The malformed prop aborts the walk before END_NODE is delivered.
        assert_eq!(visits, 0);

        // ── 4) Unterminated node name: bounded NUL scan stops the walk
        //    (pre-fix it ran off the blob).
        let mut noname = [0u8; 512];
        let struct_off = 0x40usize;
        put32(&mut noname, 0, FDT_MAGIC);
        put32(&mut noname, OFF_TOTALSIZE, 0x50);
        put32(&mut noname, OFF_STRUCT, struct_off as u32);
        put32(&mut noname, OFF_STRINGS, struct_off as u32);
        put32(&mut noname, OFF_STRINGS_SIZE, 4);
        put32(&mut noname, OFF_STRUCT_SIZE, 0x10);
        put32(&mut noname, struct_off, TOK_BEGIN_NODE);
        for i in 0..12 {
            noname[struct_off + 4 + i] = b'A'; // no NUL before the block end
        }
        assert!(crate::libfdt::fdt::init(noname.as_ptr() as usize));
        crate::libfdt::fdt::walk(&mut |_name, _props| true); // must return

        // ── 5) init_from rejections (direct, NOT via init): corrupt
        //    headers must be refused. (debug_mark is compiled out under
        //    cfg(test) so these are host-safe.)
        let bad = valid_blob();

        // 5a: struct offset beyond totalsize.
        let mut b = bad;
        put32(&mut b, OFF_STRUCT, 0x300);
        assert!(!crate::libfdt::fdt::init_from(b.as_ptr() as usize));

        // 5b: struct size runs past the blob end.
        let mut b = bad;
        put32(&mut b, OFF_STRUCT_SIZE, totalsize);
        assert!(!crate::libfdt::fdt::init_from(b.as_ptr() as usize));

        // 5c: strings offset past totalsize.
        let mut b = bad;
        put32(&mut b, OFF_STRINGS, 0x300);
        assert!(!crate::libfdt::fdt::init_from(b.as_ptr() as usize));

        // 5d: strings size runs past the blob end.
        let mut b = bad;
        put32(&mut b, OFF_STRINGS_SIZE, totalsize);
        assert!(!crate::libfdt::fdt::init_from(b.as_ptr() as usize));

        // 5e: absurd totalsize (> 4 MiB sanity cap).
        let mut b = bad;
        put32(&mut b, OFF_TOTALSIZE, 8 * 1024 * 1024);
        assert!(!crate::libfdt::fdt::init_from(b.as_ptr() as usize));

        // 5f: totalsize 0.
        let mut b = bad;
        put32(&mut b, OFF_TOTALSIZE, 0);
        assert!(!crate::libfdt::fdt::init_from(b.as_ptr() as usize));

        // ── 6) Bad magic rejected.
        let mut b = bad;
        b[0] = 0xDE;
        b[1] = 0xAD;
        b[2] = 0xBE;
        b[3] = 0xEF;
        assert!(!crate::libfdt::fdt::init_from(b.as_ptr() as usize));

        // Re-establish the valid state so later (parallel) tests that grow
        // FDT dependencies cannot observe garbage.
        assert!(crate::libfdt::fdt::init(blob.as_ptr() as usize));
    }
}
