use super::*;

#[test]
fn test_as_i64() {
    assert_eq!(Errno::Ok.as_i64(), 0);
    assert_eq!(Errno::NoMem.as_i64(), -1);
    assert_eq!(Errno::Inval.as_i64(), -2);
    assert_eq!(Errno::BadFd.as_i64(), -12);
    assert_eq!(Errno::NotEmpty.as_i64(), -17);
}
#[test]
fn test_as_str() {
    assert_eq!(Errno::Ok.as_str(), "OK");
    assert_eq!(Errno::NoMem.as_str(), "ENOMEM");
    assert_eq!(Errno::Inval.as_str(), "EINVAL");
    assert_eq!(Errno::Perm.as_str(), "EPERM");
    assert_eq!(Errno::BadFd.as_str(), "EBADF");
    assert_eq!(Errno::NotEmpty.as_str(), "ENOTEMPTY");
}
#[test]
fn test_kresult_ok() {
    let r: KResult<i32> = Ok(42);
    assert!(r.is_ok());
    assert_eq!(r, Ok(42));
}
#[test]
fn test_kresult_err() {
    let r: KResult<i32> = Err(Errno::NoMem);
    assert!(r.is_err());
    assert_eq!(r, Err(Errno::NoMem));
}
#[test]
fn test_to_posix_matches_libonyxc_errno_h() {
    // libonyxc/include/io/errno.h numbering — must stay in sync by hand,
    // there's no shared source of truth across the C/Rust boundary.
    assert_eq!(Errno::Perm.to_posix(), 1); // EPERM
    assert_eq!(Errno::NoEnt.to_posix(), 2); // ENOENT
    assert_eq!(Errno::Io.to_posix(), 5); // EIO
    assert_eq!(Errno::BadFd.to_posix(), 9); // EBADF
    assert_eq!(Errno::Child.to_posix(), 10); // ECHILD
    assert_eq!(Errno::Again.to_posix(), 11); // EAGAIN
    assert_eq!(Errno::NoMem.to_posix(), 12); // ENOMEM
    assert_eq!(Errno::Fault.to_posix(), 14); // EFAULT
    assert_eq!(Errno::Busy.to_posix(), 16); // EBUSY
    assert_eq!(Errno::Exist.to_posix(), 17); // EEXIST
    assert_eq!(Errno::NotDir.to_posix(), 20); // ENOTDIR
    assert_eq!(Errno::IsDir.to_posix(), 21); // EISDIR
    assert_eq!(Errno::Inval.to_posix(), 22); // EINVAL
    assert_eq!(Errno::NoSpace.to_posix(), 28); // ENOSPC
    assert_eq!(Errno::Pipe.to_posix(), 32); // EPIPE
    assert_eq!(Errno::Range.to_posix(), 34); // ERANGE
    assert_eq!(Errno::NoSys.to_posix(), 38); // ENOSYS
    assert_eq!(Errno::NotEmpty.to_posix(), 39); // ENOTEMPTY
    assert_eq!(Errno::Loop.to_posix(), 40); // ELOOP
}

#[test]
fn test_from_i64_round_trips_as_i64() {
    let all = [
        Errno::NoMem,
        Errno::Inval,
        Errno::NoEnt,
        Errno::Io,
        Errno::Perm,
        Errno::Range,
        Errno::NoSys,
        Errno::Busy,
        Errno::NoSpace,
        Errno::NotDir,
        Errno::IsDir,
        Errno::BadFd,
        Errno::Exist,
        Errno::Pipe,
        Errno::Overflow,
        Errno::Child,
        Errno::NotEmpty,
        Errno::Loop,
        Errno::Again,
        Errno::Fault,
    ];
    for e in all {
        assert_eq!(Errno::from_i64(e.as_i64()), Some(e));
    }
    assert_eq!(Errno::from_i64(0), None); // Ok is not an error ordinal
    assert_eq!(Errno::from_i64(-21), None); // outside the enum's range
}

#[test]
fn test_translate_syscall_result() {
    // Success and positive byte counts pass through unchanged.
    assert_eq!(Errno::translate_syscall_result(0), 0);
    assert_eq!(Errno::translate_syscall_result(4096), 4096);
    // Internal ordinal -3 (NoEnt) becomes POSIX -2 (ENOENT), not -3.
    assert_eq!(Errno::translate_syscall_result(Errno::NoEnt.as_i64()), -2);
    assert_eq!(Errno::translate_syscall_result(Errno::Io.as_i64()), -5);
    assert_eq!(Errno::translate_syscall_result(Errno::Inval.as_i64()), -22);
}

#[test]
fn test_all_errno_variants() {
    let variants = [
        (Errno::Ok, 0, "OK"),
        (Errno::NoMem, -1, "ENOMEM"),
        (Errno::Inval, -2, "EINVAL"),
        (Errno::NoEnt, -3, "ENOENT"),
        (Errno::Io, -4, "EIO"),
        (Errno::Perm, -5, "EPERM"),
        (Errno::Range, -6, "ERANGE"),
        (Errno::NoSys, -7, "ENOSYS"),
        (Errno::Busy, -8, "EBUSY"),
        (Errno::NoSpace, -9, "ENOSPC"),
        (Errno::NotDir, -10, "ENOTDIR"),
        (Errno::IsDir, -11, "EISDIR"),
        (Errno::BadFd, -12, "EBADF"),
        (Errno::Exist, -13, "EEXIST"),
        (Errno::Pipe, -14, "EPIPE"),
        (Errno::Overflow, -15, "EOVERFLOW"),
        (Errno::Child, -16, "ECHILD"),
        (Errno::NotEmpty, -17, "ENOTEMPTY"),
        (Errno::Loop, -18, "ELOOP"),
        (Errno::Again, -19, "EAGAIN"),
        (Errno::Fault, -20, "EFAULT"),
    ];
    for (e, code, name) in variants {
        assert_eq!(e.as_i64(), code, "{} code mismatch", name);
        assert_eq!(e.as_str(), name, "{} name mismatch", name);
    }
}
