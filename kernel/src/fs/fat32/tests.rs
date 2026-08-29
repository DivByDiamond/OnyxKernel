use super::*;

#[test]
fn test_fat32_name_simple() {
    let name = fat32_name_8_3(b"hello.txt");
    assert_eq!(&name[..8], b"HELLO   ");
    assert_eq!(&name[8..], b"TXT");
}

#[test]
fn test_fat32_name_no_ext() {
    let name = fat32_name_8_3(b"foo");
    assert_eq!(&name[..8], b"FOO     ");
    assert_eq!(&name[8..], b"   ");
}

#[test]
fn test_fat32_name_dot() {
    let name = fat32_name_8_3(b".");
    assert_eq!(name, [0x20u8; 11]);
}

#[test]
fn test_fat32_name_dotdot() {
    let name = fat32_name_8_3(b"..");
    assert_eq!(name, [0x20u8; 11]);
}

#[test]
fn test_fat32_name_empty() {
    let name = fat32_name_8_3(b"");
    assert_eq!(name, [0x20u8; 11]);
}

#[test]
fn test_fat32_name_makefile() {
    let name = fat32_name_8_3(b"Makefile");
    assert_eq!(&name[..8], b"MAKEFILE");
    assert_eq!(&name[8..], b"   ");
}

#[test]
fn test_fat32_name_long_ext() {
    let name = fat32_name_8_3(b"document.pdf");
    assert_eq!(&name[..8], b"DOCUMENT");
    assert_eq!(&name[8..], b"PDF");
}

#[test]
fn test_fat32_name_uppercase() {
    let name = fat32_name_8_3(b"README.TXT");
    assert_eq!(&name[..8], b"README  ");
    assert_eq!(&name[8..], b"TXT");
}

#[test]
fn test_fat32_is_eoc() {
    // SAFETY: pure predicates over u32 values; no globals or pointers touched.
    unsafe {
        assert!(is_eoc(0x0FFFFFF8));
        assert!(is_eoc(0x0FFFFFF9));
        assert!(is_eoc(0x0FFFFFFF));
        assert!(!is_eoc(0x0FFFFFF7));
        assert!(!is_eoc(0x0FFFFFF6));
        assert!(!is_eoc(2));
        assert!(!is_eoc(0));
    }
}

#[test]
fn test_fat32_valid_cluster() {
    // SAFETY: pure predicates over u32 values; no globals or pointers touched.
    unsafe {
        assert!(is_valid_cluster(2));
        assert!(is_valid_cluster(100));
        assert!(is_valid_cluster(0x0FFFFFF6));
        assert!(!is_valid_cluster(0));
        assert!(!is_valid_cluster(1));
        assert!(!is_valid_cluster(FAT32_EOC));
        assert!(!is_valid_cluster(0x0FFFFFF8));
    }
}

#[test]
fn test_fat_type_classification() {
    use super::helpers::fat_type_for_clusters;
    assert_eq!(fat_type_for_clusters(0), "FAT12");
    assert_eq!(fat_type_for_clusters(4084), "FAT12");
    assert_eq!(fat_type_for_clusters(4085), "FAT16");
    assert_eq!(fat_type_for_clusters(65524), "FAT16");
    assert_eq!(fat_type_for_clusters(65525), "FAT32");
    assert_eq!(fat_type_for_clusters(1 << 20), "FAT32");
}

#[test]
fn test_fat_entry_patch_preserves_high_nibble() {
    // Mirrors the patch logic in write::write_fat_entry: the top 4 bits
    // of an existing entry must survive a rewrite of the low 28 bits.
    let existing = 0x8000_1234u32; // 0x8 in the high nibble
    let value = 0x0FFF_FFF8u32; // EOC
    let patched = (existing & 0xF000_0000) | (value & 0x0FFF_FFFF);
    assert_eq!(patched, 0x8FFF_FFF8);
}
