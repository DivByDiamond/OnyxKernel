use crate::proc;
use onyx_core::errno::Errno;

use super::super::handler::user_ptr_ok;

pub unsafe fn sys_getuid() -> i64 {
    let p = proc::current();
    p.uid as i64
}

pub unsafe fn sys_getgid() -> i64 {
    let p = proc::current();
    p.gid as i64
}

pub unsafe fn sys_uname(buf: u64) -> i64 {
    if !user_ptr_ok(buf, 390) {
        return Errno::Inval.as_i64();
    }
    let out = buf as *mut u8;
    let sysname = b"Onyx\0";
    let nodename = b"onyx\0";
    let release = b"0.3.0\0";
    let version = b"#1 Onyx Kernel 0.3.0\0";
    let machine = b"riscv64\0";
    let mut off = 0;
    for &b in sysname { *out.add(off) = b; off += 1; } let sz = 65usize;
    off = sz;
    for &b in nodename { *out.add(off) = b; off += 1; } off = sz * 2;
    for &b in release { *out.add(off) = b; off += 1; } off = sz * 3;
    for &b in version { *out.add(off) = b; off += 1; } off = sz * 4;
    for &b in machine { *out.add(off) = b; off += 1; }
    0
}
