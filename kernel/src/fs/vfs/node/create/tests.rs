use super::*;

#[test]
fn test_split_parent_root() {
    // SAFETY: split_parent performs only bounds-checked slice arithmetic.
    unsafe {
        let (parent, name) = split_parent(b"/foo");
        assert_eq!(parent, b"");
        assert_eq!(name, b"foo");
    }
}

#[test]
fn test_split_parent_nested() {
    // SAFETY: split_parent performs only bounds-checked slice arithmetic.
    unsafe {
        let (parent, name) = split_parent(b"/foo/bar/baz");
        assert_eq!(parent, b"foo/bar");
        assert_eq!(name, b"baz");
    }
}

#[test]
fn test_split_parent_no_slash() {
    // SAFETY: split_parent performs only bounds-checked slice arithmetic.
    unsafe {
        let (parent, name) = split_parent(b"foo");
        assert_eq!(parent, b"");
        assert_eq!(name, b"foo");
    }
}

#[test]
fn test_split_parent_trailing_slash() {
    // SAFETY: split_parent performs only bounds-checked slice arithmetic.
    unsafe {
        let (parent, name) = split_parent(b"/foo/bar/");
        assert_eq!(parent, b"foo/bar");
        assert_eq!(name, b"");
    }
}

#[test]
fn test_split_parent_single_component() {
    // SAFETY: split_parent performs only bounds-checked slice arithmetic.
    unsafe {
        let (parent, name) = split_parent(b"/");
        assert_eq!(parent, b"");
        assert_eq!(name, b"");
    }
}

#[test]
fn test_split_parent_deep_nested() {
    // SAFETY: split_parent performs only bounds-checked slice arithmetic.
    unsafe {
        let (parent, name) = split_parent(b"/a/b/c/d/e/f");
        assert_eq!(parent, b"a/b/c/d/e");
        assert_eq!(name, b"f");
    }
}

#[test]
fn test_split_parent_empty() {
    // SAFETY: split_parent performs only bounds-checked slice arithmetic.
    unsafe {
        let (parent, name) = split_parent(b"");
        assert_eq!(parent, b"");
        assert_eq!(name, b"");
    }
}
