#![cfg(test)]

use super::*;

pub mod onx;
pub mod onyfs;
pub mod snapshot;

#[test]
fn test_name_to_83() {
    assert_eq!(name_to_83(b"kernel.elf").unwrap(), *b"KERNEL  ELF");
}

#[test]
fn test_matches_83() {
    let dirent: &[u8] = b"KERNEL  ELF";
    assert!(matches_83(dirent, b"kernel.elf"));
}
