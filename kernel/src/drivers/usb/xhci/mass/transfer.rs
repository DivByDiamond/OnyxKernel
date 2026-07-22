use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::{Cbw, Csw, MassStorageDev, CBW_SIG, CBW_SIZE, G_MASS, G_NMASS};
use crate::drivers::usb::xhci::ring;

unsafe fn bulk_send(ep: u8, buf: *const u8, len: u32) -> KResult<()> {
    let ctx = &raw mut super::super::G_XHCI;
    let pa = buf as u64;
    let mut trb = ring::Trb::zero();
    trb.params[0] = pa as u32;
    trb.params[1] = (pa >> 32) as u32;
    trb.params[2] = len;
    trb.set_type(ring::TRB_NORMAL);
    trb.set_flags(ring::TRB_IOC | ring::TRB_CH);
    let ring = &mut *(*ctx).xfer_rings[ep as usize];
    ring::enqueue_trb(ring, &trb);
    ring::ring_doorbell((*ctx).slot, ep);
    let mut timeout = 1_000_000u32;
    let er = &mut (*ctx).event_ring;
    while timeout > 0 {
        if let Some(ev) = ring::poll_event(er) {
            let t = (ev.params[3] >> 10) & 0x3F;
            if t == ring::TRB_TRANSFER_EVENT {
                return Ok(());
            }
        }
        timeout -= 1;
    }
    Err(Errno::Io)
}

unsafe fn bulk_recv(ep: u8, buf: *mut u8, len: u32) -> KResult<()> {
    let ctx = &raw mut super::super::G_XHCI;
    let pa = buf as u64;
    let mut trb = ring::Trb::zero();
    trb.params[0] = pa as u32;
    trb.params[1] = (pa >> 32) as u32;
    trb.params[2] = len;
    trb.set_type(ring::TRB_NORMAL);
    trb.set_flags(ring::TRB_IOC | ring::TRB_CH);
    let ring = &mut *(*ctx).xfer_rings[ep as usize];
    ring::enqueue_trb(ring, &trb);
    ring::ring_doorbell((*ctx).slot, ep);
    let mut timeout = 1_000_000u32;
    let er = &mut (*ctx).event_ring;
    while timeout > 0 {
        if let Some(ev) = ring::poll_event(er) {
            let t = (ev.params[3] >> 10) & 0x3F;
            if t == ring::TRB_TRANSFER_EVENT {
                return Ok(());
            }
        }
        timeout -= 1;
    }
    Err(Errno::Io)
}

pub unsafe fn get_max_lun(idx: usize) -> KResult<u8> {
    let dev = (*G_MASS.as_ptr().add(idx)).as_ref().ok_or(Errno::NoEnt)?;
    let cbw_pa = pmm::alloc_zero()? as *mut Cbw;
    let mut cbw = Cbw {
        sig: CBW_SIG,
        tag: 0x10000000,
        xfer_len: 1,
        flags: 0x80,
        lun: 0,
        cb_len: 1,
        cdb: [0; 16],
    };
    cbw.cdb[0] = 0xFE;
    ptr::write(cbw_pa, cbw);
    bulk_send(dev.bulk_out, cbw_pa as *mut u8, CBW_SIZE)?;
    let mut data: u8 = 0;
    bulk_recv(dev.bulk_in, &mut data as *mut u8, 1)?;
    let max_lun = data;
    Ok(max_lun)
}

pub unsafe fn scsi_test_unit_ready(idx: usize) -> KResult<()> {
    let dev = (*G_MASS.as_ptr().add(idx)).as_ref().ok_or(Errno::NoEnt)?;
    let cbw_pa = pmm::alloc_zero()? as *mut Cbw;
    let csw_pa = pmm::alloc_zero()? as *mut Csw;
    let cbw = Cbw {
        sig: CBW_SIG,
        tag: 0x10000001,
        xfer_len: 0,
        flags: 0x80,
        lun: 0,
        cb_len: 6,
        cdb: [0; 16],
    };
    ptr::write(cbw_pa, cbw);
    bulk_send(dev.bulk_out, cbw_pa as *mut u8, CBW_SIZE)?;
    let csw = ptr::read_unaligned(csw_pa as *const Csw);
    if csw.status != 0 {
        return Err(Errno::Io);
    }
    Ok(())
}

pub unsafe fn scsi_read_capacity10(idx: usize) -> KResult<(u32, u32)> {
    let dev = (*G_MASS.as_ptr().add(idx)).as_ref().ok_or(Errno::NoEnt)?;
    let cbw_pa = pmm::alloc_zero()? as *mut Cbw;
    let csw_pa = pmm::alloc_zero()? as *mut Csw;
    let mut data: [u8; 8] = [0; 8];
    let mut cbw = Cbw {
        sig: CBW_SIG,
        tag: 0x10000002,
        xfer_len: 8,
        flags: 0x80,
        lun: 0,
        cb_len: 10,
        cdb: [0; 16],
    };
    cbw.cdb[0] = 0x25;
    ptr::write(cbw_pa, cbw);
    bulk_send(dev.bulk_out, cbw_pa as *mut u8, CBW_SIZE)?;
    bulk_recv(dev.bulk_in, data.as_mut_ptr(), 8)?;
    let csw = ptr::read_unaligned(csw_pa as *const Csw);
    if csw.status != 0 {
        return Err(Errno::Io);
    }
    let last_lba = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let blk_size = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    Ok((last_lba, blk_size))
}

pub unsafe fn scsi_read10(idx: usize, lba: u32, count: u16, buf: &mut [u8]) -> KResult<()> {
    let dev = (*G_MASS.as_ptr().add(idx)).as_ref().ok_or(Errno::NoEnt)?;
    let cbw_pa = pmm::alloc_zero()? as *mut Cbw;
    let csw_pa = pmm::alloc_zero()? as *mut Csw;
    let xfer = (count as u32) * dev.block_size;
    let mut cbw = Cbw {
        sig: CBW_SIG,
        tag: 0x10000003,
        xfer_len: xfer,
        flags: 0x80,
        lun: 0,
        cb_len: 10,
        cdb: [0; 16],
    };
    cbw.cdb[0] = 0x28;
    cbw.cdb[2] = (lba >> 24) as u8;
    cbw.cdb[3] = (lba >> 16) as u8;
    cbw.cdb[4] = (lba >> 8) as u8;
    cbw.cdb[5] = lba as u8;
    cbw.cdb[7] = (count >> 8) as u8;
    cbw.cdb[8] = count as u8;
    ptr::write(cbw_pa, cbw);
    bulk_send(dev.bulk_out, cbw_pa as *mut u8, CBW_SIZE)?;
    bulk_recv(dev.bulk_in, buf.as_mut_ptr(), xfer)?;
    let csw = ptr::read_unaligned(csw_pa as *const Csw);
    if csw.status != 0 {
        return Err(Errno::Io);
    }
    Ok(())
}

pub unsafe fn scsi_write10(idx: usize, lba: u32, count: u16, buf: &[u8]) -> KResult<()> {
    let dev = (*G_MASS.as_ptr().add(idx)).as_ref().ok_or(Errno::NoEnt)?;
    let cbw_pa = pmm::alloc_zero()? as *mut Cbw;
    let csw_pa = pmm::alloc_zero()? as *mut Csw;
    let xfer = (count as u32) * dev.block_size;
    let mut cbw = Cbw {
        sig: CBW_SIG,
        tag: 0x10000004,
        xfer_len: xfer,
        flags: 0x00,
        lun: 0,
        cb_len: 10,
        cdb: [0; 16],
    };
    cbw.cdb[0] = 0x2A;
    cbw.cdb[2] = (lba >> 24) as u8;
    cbw.cdb[3] = (lba >> 16) as u8;
    cbw.cdb[4] = (lba >> 8) as u8;
    cbw.cdb[5] = lba as u8;
    cbw.cdb[7] = (count >> 8) as u8;
    cbw.cdb[8] = count as u8;
    ptr::write(cbw_pa, cbw);
    bulk_send(dev.bulk_out, cbw_pa as *mut u8, CBW_SIZE)?;
    bulk_send(dev.bulk_out, buf.as_ptr(), xfer)?;
    let csw = ptr::read_unaligned(csw_pa as *const Csw);
    if csw.status != 0 {
        return Err(Errno::Io);
    }
    Ok(())
}
