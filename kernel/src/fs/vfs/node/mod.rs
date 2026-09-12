//! Namespace node operations: creation, removal, links and the vnode core.
pub mod create;
pub mod dir;
pub mod symlink;
pub mod unlink;
pub mod vnode;

/// Split a path like "/foo/bar/baz" (or its mount-relative form
/// "foo/bar/baz") into ("foo/bar", "baz"). A leading '/' is stripped;
/// a path with no remaining '/' yields ("", name). Shared by `create`,
/// `mkdir` and `symlink` to find the parent directory.
///
/// # Safety
///
/// No unsafe operations inside; only bounds-checked slice arithmetic. The
/// unsafe signature mirrors the historical per-module copies.
pub(crate) unsafe fn split_parent(path: &[u8]) -> (&[u8], &[u8]) {
    let p = if !path.is_empty() && path[0] == b'/' {
        &path[1..]
    } else {
        path
    };
    match p.iter().rposition(|&b| b == b'/') {
        Some(idx) => (&p[..idx], &p[idx + 1..]),
        None => (&[], p),
    }
}
