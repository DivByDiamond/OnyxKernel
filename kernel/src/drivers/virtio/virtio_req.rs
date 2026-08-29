//! virtio-blk request submission + polled I/O.
use crate::drivers::virtio::*;
use core::ptr;
use core::sync::atomic::{Ordering, fence};
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

/// Perform one complete virtio-blk request: copy data in (for writes),
/// submit the 3-descriptor chain, poll `used` until completion, copy data
/// out (for IN requests).
///
/// All of it runs under the per-device queue lock `G_QLOCK[dev_idx]`
/// (audit fix): descriptor table, avail/used rings, `req_buf` and
/// `last_used` are single-instance state per device; two harts issuing
/// concurrent I/O to the same disk previously raced on all of them.
///
/// # Safety
///
/// `dev_idx` must be an initialized virtio-blk device index (device present
/// in G_DEVS with queues set up); caller must be in kernel context
/// (SIE = 0, see crate::sync — the spinlock only spins cross-hart).
/// For `VIRTIO_BLK_T_IN`, `buf` must be writable for `VIRTIO_BLK_SECTOR`
/// bytes; for `VIRTIO_BLK_T_OUT` it must be readable for the same length.
unsafe fn request(dev_idx: usize, req_type: u32, lba: u64, buf: *mut u8) -> KResult<()> {
    // SAFETY: dev_idx is bounds-checked against VIRTIO_MAX_DEVS (public read/write wrappers already validated it against G_NDEVS); G_QLOCK[dev_idx] parallels G_DEVS.
    unsafe {
        if dev_idx >= VIRTIO_MAX_DEVS {
            return Err(Errno::NoEnt);
        }
        let pd = &raw mut G_DEVS;
        let dev = &mut (*pd)[dev_idx];
        if dev.req_buf.is_null() || dev.base == 0 {
            return Err(Errno::Io);
        }
        // Serialize the whole request lifecycle: copy-in, submission,
        // polling and copy-out share the per-device desc/avail/used rings,
        // the single req_buf and last_used. SIE=0 prevents same-hart
        // preemption (see crate::sync); cross-hart callers block here,
        // which is exactly the intended serialization.
        G_QLOCK[dev_idx].lock();
        let r = if req_type == VIRTIO_BLK_T_IN {
            let rr = submit_and_wait_locked(dev, req_type, lba);
            // SAFETY: copy-out happens while the queue lock is held, so no other request can overwrite req_buf in between; buf is writable for VIRTIO_BLK_SECTOR bytes per the caller contract.
            if rr.is_ok() {
                ptr::copy_nonoverlapping((*dev.req_buf).data.as_ptr(), buf, VIRTIO_BLK_SECTOR);
            }
            rr
        } else {
            // SAFETY: copy of VIRTIO_BLK_SECTOR bytes into the device req buffer under the queue lock; buf is readable for 512 bytes per the caller contract.
            ptr::copy_nonoverlapping(
                buf as *const u8,
                (*dev.req_buf).data.as_mut_ptr(),
                VIRTIO_BLK_SECTOR,
            );
            submit_and_wait_locked(dev, req_type, lba)
        };
        G_QLOCK[dev_idx].unlock();
        r
    }
}

/// Submit a 3-descriptor chain (header, 512-byte data, status) on the
/// device's queue, kick the device and poll `used.idx` until the request
/// completes. Must be called with `G_QLOCK[dev_idx]` held (see `request`).
///
/// # Safety
///
/// `dev` must be a fully initialized device (`init`/`setup_queue` done:
/// req_buf/desc/avail/used non-null, queue ready); caller must hold the
/// device's queue lock.
unsafe fn submit_and_wait_locked(
    dev: &mut VirtioBlkDev,
    req_type: u32,
    sector: u64,
) -> KResult<()> {
    // SAFETY: caller holds the per-device queue lock, so desc/avail/used/req_buf are exclusively owned here; desc slots 0..=2 < VIRTQ_SIZE; avail slot masked % VIRTQ_SIZE per spec; volatiles + SeqCst fence order ring entry before idx bump/notify.
    unsafe {
        (*dev.req_buf).req_type = req_type;
        (*dev.req_buf).reserved = 0;
        (*dev.req_buf).sector = sector;
        (*dev.req_buf).status = 0xFF;
        let req_pa = dev.req_buf as u64;
        let data_off = 16;
        let status_off = data_off + VIRTIO_BLK_SECTOR;
        (*dev.desc.add(0)) = VqDesc {
            addr: req_pa,
            len: 16,
            flags: VQ_DESC_F_NEXT,
            next: 1,
        };
        let data_flags = if req_type == VIRTIO_BLK_T_IN {
            VQ_DESC_F_NEXT | VQ_DESC_F_WRITE
        } else {
            VQ_DESC_F_NEXT
        };
        (*dev.desc.add(1)) = VqDesc {
            addr: req_pa + data_off as u64,
            len: VIRTIO_BLK_SECTOR as u32,
            flags: data_flags,
            next: 2,
        };
        (*dev.desc.add(2)) = VqDesc {
            addr: req_pa + status_off as u64,
            len: 1,
            flags: VQ_DESC_F_WRITE,
            next: 0,
        };
        let idx = core::ptr::read_volatile(core::ptr::addr_of!((*dev.avail).idx));
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!((*dev.avail).ring[(idx as usize) % VIRTQ_SIZE]),
            0,
        );
        fence(Ordering::SeqCst);
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!((*dev.avail).idx),
            idx.wrapping_add(1),
        );
        reg_w(dev.base, R_QUEUE_NOTIFY, 0);
        let used_idx_ptr = core::ptr::addr_of!((*dev.used).idx);
        let mut spins = 0u64;
        loop {
            if core::ptr::read_volatile(used_idx_ptr) != dev.last_used {
                break;
            }
            spins += 1;
            if spins == 500_000 {
                crate::kwrn!(
                    "virtio",
                    "req type=%d sector=%d spins=%d avail.idx=%d used.idx=%d queue_ready?",
                    Arg::from(req_type),
                    Arg::from(sector),
                    Arg::from(spins),
                    Arg::from(u32::from(core::ptr::read_volatile(core::ptr::addr_of!(
                        (*dev.avail).idx
                    )))),
                    Arg::from(u32::from(core::ptr::read_volatile(used_idx_ptr)))
                );
            }
        }
        dev.last_used = core::ptr::read_volatile(used_idx_ptr);
        if (*dev.req_buf).status == VIRTIO_BLK_S_OK {
            Ok(())
        } else {
            Err(Errno::Io)
        }
    }
}

/// Full single-sector read under the per-device queue lock.
///
/// # Safety
///
/// `buf` must point to at least `VIRTIO_BLK_SECTOR` (512) bytes of writable
/// memory; `dev_idx` must be < `virtio::count()` (device fully initialized).
pub unsafe fn read(dev_idx: usize, lba: u64, buf: *mut u8) -> KResult<()> {
    // SAFETY: dev_idx < virtio::count() is validated by callers (devfs/blk.rs, onyxfs); request() performs the bounds check too.
    unsafe { request(dev_idx, VIRTIO_BLK_T_IN, lba, buf) }
}

/// Full single-sector write.
///
/// # Safety
///
/// `buf` must point to at least `VIRTIO_BLK_SECTOR` (512) bytes of readable
/// memory; `dev_idx` must satisfy the `read` contract.
pub unsafe fn write(dev_idx: usize, lba: u64, buf: *const u8) -> KResult<()> {
    // SAFETY: dev_idx contract per read; buf readable for 512 bytes per the doc contract above.
    unsafe { request(dev_idx, VIRTIO_BLK_T_OUT, lba, buf as *mut u8) }
}

/// Read `n_sectors` consecutive 512-byte sectors starting at `lba` into `buf`.
/// `buf` must point to at least `n_sectors * 512` bytes of writable memory.
///
/// MVP implementation: loops over `read()` for each sector. The
/// infrastructure is here so a future scatter-gather optimization can replace
/// the loop with a single batched virtio-blk request.
///
/// # Safety
///
/// `dev_idx` must satisfy the `read` contract; `buf` must have room for
/// `n_sectors * 512` writable bytes (stated above).
pub unsafe fn read_multi(dev_idx: usize, lba: u64, n_sectors: u32, buf: *mut u8) -> KResult<()> {
    // SAFETY: per-iteration read() contract upheld: buf has room for n_sectors * 512 bytes per the doc contract, so each buf.add(i * 512) slice is in bounds.
    unsafe {
        for i in 0u32..n_sectors {
            read(
                dev_idx,
                lba + i as u64,
                buf.add((i as usize) * VIRTIO_BLK_SECTOR),
            )?;
        }
        Ok(())
    }
}

/// Write `n_sectors` consecutive 512-byte sectors starting at `lba` from `buf`.
/// `buf` must point to at least `n_sectors * 512` bytes of readable memory.
///
/// MVP implementation: loops over `write()` for each sector. Like
/// `read_multi`, this is the seam where a future scatter-gather optimization
/// would issue a single batched virtio-blk request.
///
/// # Safety
///
/// `dev_idx` must satisfy the `write` contract; `buf` must provide
/// `n_sectors * 512` readable bytes (stated above).
pub unsafe fn write_multi(dev_idx: usize, lba: u64, n_sectors: u32, buf: *const u8) -> KResult<()> {
    // SAFETY: per-iteration write() contract upheld: buf provides n_sectors * 512 readable bytes per the doc contract, so each buf.add(i * 512) slice is in bounds.
    unsafe {
        for i in 0u32..n_sectors {
            write(
                dev_idx,
                lba + i as u64,
                buf.add((i as usize) * VIRTIO_BLK_SECTOR),
            )?;
        }
        Ok(())
    }
}
