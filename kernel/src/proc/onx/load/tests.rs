use super::*;
use onyx_core::formats::{ONX_MAGIC, ONX_VERSION_1, OnxSegment};

fn header(flags: u32) -> OnxHeader {
    OnxHeader {
        magic: ONX_MAGIC,
        version: ONX_VERSION_1,
        entry: USER_BASE,
        nsegs: 1,
        flags: 0,
        segs: alloc::vec![OnxSegment {
            vaddr: USER_BASE,
            filesz: 4,
            memsz: 4096,
            offset: OnxHeader::V1_HEADER_SIZE as u32,
            flags,
            align: 4096,
            reserved: 0,
            compressed_size: 0,
        }],
    }
}

#[test]
fn invalid_segment_permissions_are_rejected_before_mapping() {
    let invalid = [
        0,
        PTE_W,
        PTE_W | PTE_X,
        PTE_V | PTE_W, // Old onyx-ld text flags (0x5).
        PTE_R | PTE_U,
        PTE_R | (1 << PTE_PPN_SHIFT),
    ];
    for flags in invalid {
        assert_eq!(validate_image(&header(flags as u32)), Err(Errno::Inval));
    }
}

#[test]
fn onyx_read_write_execute_permissions_remain_accepted() {
    for flags in [
        PTE_R,
        PTE_R | PTE_W,
        PTE_X,
        PTE_R | PTE_X,
        PTE_R | PTE_W | PTE_X,
    ] {
        assert_eq!(validate_image(&header(flags as u32)), Ok(()));
    }
}
