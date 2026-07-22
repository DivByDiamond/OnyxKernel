use super::super::consts::*;
use core::arch::asm;

#[inline]
pub unsafe fn chan_create() -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHAN_CREATE, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chan_connect(chan_id: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHAN_CONNECT, in("a0") chan_id, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chan_send(chan_id: u32, buf: *const u8, len: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHAN_SEND, in("a0") chan_id, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chan_recv(chan_id: u32, buf: *mut u8, len: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHAN_RECV, in("a0") chan_id, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chan_close(chan_id: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHAN_CLOSE, in("a0") chan_id, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chan_create_named(name: *const u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHAN_CREATE_NAMED, in("a0") name as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chan_open(name: *const u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHAN_OPEN, in("a0") name as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn utimens(path: *const u8, times: *const u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_UTIMENS, in("a0") path as usize, in("a1") times as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn truncate2(path: *const u8, length: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_TRUNCATE2, in("a0") path as usize, in("a1") length, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn ftruncate(fd: u64, length: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_FTRUNCATE, in("a0") fd, in("a1") length, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn readlink(path: *const u8, buf: *mut u8, bufsiz: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_READLINK, in("a0") path as usize, in("a1") buf as usize, in("a2") bufsiz, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn symlink(target: *const u8, linkpath: *const u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SYMLINK, in("a0") target as usize, in("a1") linkpath as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chmod(path: *const u8, mode: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHMOD, in("a0") path as usize, in("a1") mode, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn fchmod(fd: u64, mode: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_FCHMOD, in("a0") fd, in("a1") mode, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn chown(path: *const u8, uid: u64, gid: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_CHOWN, in("a0") path as usize, in("a1") uid, in("a2") gid, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn fchown(fd: u64, uid: u64, gid: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_FCHOWN, in("a0") fd, in("a1") uid, in("a2") gid, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn fsync(fd: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_FSYNC, in("a0") fd, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn snapshot_create(name: *const u8) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SNAPSHOT_CREATE, in("a0") name as usize, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn snapshot_rollback(id: u32) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SNAPSHOT_ROLLBACK, in("a0") id, lateout("a0") ret);
    ret
}
#[inline]
pub unsafe fn snapshot_list(buf: *mut u8, len: u64) -> i64 {
    let ret;
    asm!("ecall", in("a7") SYS_SNAPSHOT_LIST, in("a0") buf as usize, in("a1") len, lateout("a0") ret);
    ret
}
