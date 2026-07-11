use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::ring;

pub(crate) const MAX_MASS: usize = 4;

pub struct MassStorageDev {
    pub slot_id: u8,
    pub bulk_in: u8,
    pub bulk_out: u8,
    pub max_lun: u8,
    pub block_size: u32,
    pub block_count: u64,
}

pub(crate) static mut G_MASS: [Option<MassStorageDev>; MAX_MASS] = [const { None }; MAX_MASS];
pub(crate) static mut G_NMASS: usize = 0;

#[repr(C, packed)]
struct Cbw {
    sig: u32,
    tag: u32,
    xfer_len: u32,
    flags: u8,
    lun: u8,
    cb_len: u8,
    cdb: [u8; 16],
}

#[repr(C, packed)]
struct Csw {
    sig: u32,
    tag: u32,
    residue: u32,
    status: u8,
}

const CBW_SIG: u32 = 0x43425355;
const CBW_SIZE: u32 = 31;

pub unsafe fn probe(slot_id: u8, config_data: &[u8]) -> KResult<u8> {
    let mut i = 0;
    while i + 8 < config_data.len() {
        let len = config_data[i] as usize;
        if len < 2 {
            break;
        }
        let desc_type = config_data[i + 1];
        if desc_type == 4 && len >= 8 {
            let if_class = config_data[i + 5];
            let if_sub = config_data[i + 6];
            let if_proto = config_data[i + 7];
            if if_class == 0x08 && if_sub == 0x50 && if_proto == 0x50 {
                let mut bul_in = 0;
                let mut bul_out = 0;
                let mut j = i + len;
                while j + 1 < config_data.len() && j < i + 80 {
                    let elen = config_data[j] as usize;
                    if elen < 3 {
                        break;
                    }
                    let etype = config_data[j + 1];
                    if etype == 5 && elen >= 7 {
                        let ep_addr = config_data[j + 2];
                        let ep_attr = config_data[j + 3];
                        if ep_attr & 0x03 == 2 {
                            let _mps = (config_data[j + 5] as u16) << 8 | config_data[j + 4] as u16;
                            if ep_addr & 0x80 != 0 {
                                bul_in = ep_addr;
                            } else {
                                bul_out = ep_addr;
                            }
                        }
                    }
                    j += elen;
                }
                if bul_in != 0 && bul_out != 0 {
                    let nm = ptr::read(&raw const G_NMASS);
                    if nm >= MAX_MASS {
                        return Err(Errno::NoMem);
                    }
                    let dev = MassStorageDev {
                        slot_id,
                        bulk_in: bul_in & 0x0F,
                        bulk_out: bul_out & 0x0F,
                        max_lun: 0,
                        block_size: 512,
                        block_count: 0,
                    };
                    let p = &raw mut G_MASS;
                    ptr::write((*p).as_mut_ptr().add(nm), Some(dev));
                    ptr::write(&raw mut G_NMASS, nm + 1);
                    return Ok(nm as u8);
                }
            }
        }
        i += len;
    }
    Err(Errno::NoEnt)
}

unsafe fn bulk_send(ep: u8, buf: *const u8, len: u32) -> KResult<()> {
    let ctx = &raw mut super::G_XHCI;
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
    let ctx = &raw mut super::G_XHCI;
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
