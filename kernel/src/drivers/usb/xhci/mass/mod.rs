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
pub(super) struct Cbw {
    sig: u32,
    tag: u32,
    xfer_len: u32,
    flags: u8,
    lun: u8,
    cb_len: u8,
    cdb: [u8; 16],
}

#[repr(C, packed)]
pub(super) struct Csw {
    sig: u32,
    tag: u32,
    residue: u32,
    status: u8,
}

pub(super) const CBW_SIG: u32 = 0x43425355;
pub(super) const CBW_SIZE: u32 = 31;

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
