//! Privileged static-IP/interface selection and bounded UDP demo operations.
use super::handler::user_ptr_ok;
use crate::{mm::vmm, net, proc};
use onyx_core::errno::{Errno, KResult};

unsafe fn copyin<const N: usize>(ptr: u64) -> KResult<[u8; N]> {
    if !user_ptr_ok(ptr, N as u64) {
        return Err(Errno::Inval);
    }
    let mut data = [0; N];
    // SAFETY: helper validates each user page before copying into the sized array.
    unsafe { vmm::copy_from_user(proc::current().root_pa, data.as_mut_ptr(), ptr, N)?; }
    Ok(data)
}

unsafe fn copyout(ptr: u64, data: &[u8]) -> KResult<()> {
    if !user_ptr_ok(ptr, data.len() as u64) {
        return Err(Errno::Inval);
    }
    // SAFETY: helper validates writable user pages; source is a valid slice.
    unsafe { vmm::copy_to_user(proc::current().root_pa, ptr, data.as_ptr(), data.len()) }
}

/// # Safety
/// Privileged syscall path; copy helpers validate all user pointers.
pub(super) unsafe fn sys_net_config(ptr: u64, write: u64) -> i64 {
    // SAFETY: copyin/out validate buffers and config::apply serializes IO.
    let result = unsafe {
        match write {
            0 => net::config::snapshot(None).and_then(|data| copyout(ptr, &data)),
            2 => copyin::<4>(ptr).and_then(|index| net::config::snapshot(Some(u32::from_le_bytes(index) as usize))).and_then(|data| copyout(ptr, &data)),
            1 => copyin::<{ net::config::CONFIG_SIZE }>(ptr).and_then(|data| net::config::apply(&data)),
            _ => Err(Errno::Inval),
        }
    };
    result.map_or_else(Errno::as_i64, |()| 0)
}

/// # Safety
/// Privileged syscall path, no user pointers.
pub(super) unsafe fn sys_udp_bind(port: u64) -> i64 {
    if port == 0 || port > u16::MAX as u64 {
        return Errno::Inval.as_i64();
    }
    // ownership::bind_udp takes the NET lock itself and touches no user pointer.
    net::ownership::bind_udp(port as u16, proc::current_pid()).map_or_else(Errno::as_i64, |id| id as i64)
}

/// # Safety
/// Privileged syscall path; data length is bounded and pointers are copied.
pub(super) unsafe fn sys_udp_sendto(ip: u64, port: u64, ptr: u64, len: u64) -> i64 {
    if port == 0 || port > u16::MAX as u64 || len == 0 || len > 1024 || !user_ptr_ok(ptr, len) {
        return Errno::Inval.as_i64();
    }
    // SAFETY: both user buffers are copied through page-validating helpers;
    // the kernel payload is bounded to one small Ethernet datagram.
    unsafe {
        let ip = match copyin::<4>(ip) {
            Ok(ip) => ip,
            Err(error) => return error.as_i64(),
        };
        let mut data = [0; 1024];
        if let Err(error) = vmm::copy_from_user(proc::current().root_pa, data.as_mut_ptr(), ptr, len as usize) {
            return error.as_i64();
        }
        net::udp::udp_sendto(ip, port as u16, &data[..len as usize]).map_or_else(Errno::as_i64, |()| len as i64)
    }
}

/// # Safety
/// Privileged syscall path; sized kernel staging buffer prevents user faults.
pub(super) unsafe fn sys_udp_recv(id: u64, ptr: u64, len: u64) -> i64 {
    if id >= net::udp::MAX_UDP_SOCKS as u64 || len == 0 || len > 1024 || !user_ptr_ok(ptr, len) {
        return Errno::Inval.as_i64();
    }
    // SAFETY: poll/recv serialize rings; copyout validates writable user pages.
    unsafe {
        let mut data = [0; 1024];
        net::poll();
        match net::ownership::udp(id as usize,proc::current_pid(),||net::udp::udp_recv(id as usize, &mut data[..len as usize])) {
            Ok(n) => copyout(ptr, &data[..n]).map_or_else(Errno::as_i64, |()| n as i64),
            Err(Errno::NoEnt) => 0,
            Err(error) => error.as_i64(),
        }
    }
}

/// # Safety
/// Privileged syscall path; socket index is checked before closing.
pub(super) unsafe fn sys_udp_close(id: u64) -> i64 {
    if id >= net::udp::MAX_UDP_SOCKS as u64 {
        return Errno::Inval.as_i64();
    }
    // SAFETY: udp_close serializes the bounded slot table.
    unsafe { net::ownership::udp(id as usize,proc::current_pid(),||{net::udp::udp_close(id as usize);Ok(())}).map_or_else(Errno::as_i64,|()|0) }
}
