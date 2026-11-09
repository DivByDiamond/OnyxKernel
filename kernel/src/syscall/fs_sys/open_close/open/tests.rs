use super::resolve_open_path;

#[test]
fn relative_path_uses_current_directory() {
    let mut joined = [0; 512];
    assert_eq!(
        resolve_open_path(b"internet.txt", b"/", &mut joined),
        Some(b"/internet.txt".as_slice())
    );

    let mut joined = [0; 512];
    assert_eq!(
        resolve_open_path(b"docs/page.txt", b"/users/root", &mut joined),
        Some(b"/users/root/docs/page.txt".as_slice())
    );
}

#[test]
fn absolute_path_is_left_unchanged() {
    let mut joined = [0; 512];
    assert_eq!(
        resolve_open_path(b"/tmp/file.txt", b"/users/root", &mut joined),
        Some(b"/tmp/file.txt".as_slice())
    );
}

#[test]
fn empty_path_and_invalid_current_directory_are_rejected() {
    let mut joined = [0; 512];
    assert!(resolve_open_path(b"", b"/", &mut joined).is_none());
    assert!(resolve_open_path(b"file", b"", &mut joined).is_none());
}
