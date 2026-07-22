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
