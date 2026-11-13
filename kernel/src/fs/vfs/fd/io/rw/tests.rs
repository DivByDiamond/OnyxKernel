use super::write_start;
use crate::syscall::abi::{O_APPEND, O_WRONLY};

#[test]
fn appending_descriptor_writes_at_the_end_of_the_file() {
    assert_eq!(write_start(O_WRONLY | O_APPEND, 15, 195), 195);
}

#[test]
fn plain_descriptor_keeps_its_own_position() {
    assert_eq!(write_start(O_WRONLY, 15, 195), 15);
}
