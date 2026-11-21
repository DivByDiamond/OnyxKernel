use super::*;

#[test]
fn nested_parent_dirs_are_created_once() {
    let mut dirs = vec![DirNode::root()];
    let mut next_ino = 2;
    ensure_parent_dirs(&mut dirs, "/usr/share/music/a.dfpwm", &mut next_ino);
    ensure_parent_dirs(&mut dirs, "/usr/share/music/b.dfpwm", &mut next_ino);
    ensure_parent_dirs(&mut dirs, "/usr/share/doc/c.txt", &mut next_ino);
    // usr, share, music, doc: one inode each, no duplicate names in any directory.
    assert_eq!(dirs.len(), 5);
    assert_eq!(next_ino, 6);
    for dir in &dirs {
        let mut names: Vec<&str> = dir.entries.iter().map(|(n, _, _)| n.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), dir.entries.len());
    }
}
