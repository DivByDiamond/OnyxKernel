use super::*;
use crate::fs::vfs::VfsFd;

#[test]
fn spawn_inherits_only_the_standard_stream_bindings() {
    let mut parent = Box::new(Proc::new());
    let mut child = Box::new(Proc::new());
    for slot in [0usize, 1, 2, 3, 4] {
        parent.fds[slot] = VfsFd {
            used: true,
            ino: 100 + slot as u32,
            ..VfsFd::default()
        };
    }
    inherit_stdio(&parent, &mut child);
    for slot in 0..3 {
        assert!(child.fds[slot].used, "stdio slot {slot} is inherited");
        assert_eq!(child.fds[slot].ino, 100 + slot as u32);
    }
    assert!(
        !child.fds[3].used && !child.fds[4].used,
        "other descriptors stay private"
    );
}

#[test]
fn spawn_keeps_the_console_default_when_nothing_is_bound() {
    let parent = Box::new(Proc::new());
    let mut child = Box::new(Proc::new());
    inherit_stdio(&parent, &mut child);
    assert!(!child.fds[0].used && !child.fds[1].used && !child.fds[2].used);
}
