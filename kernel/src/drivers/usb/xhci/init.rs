use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::regs;
use super::ring;
use super::XhciCtx;
use super::G_XHCI;

pub unsafe fn init(base: usize) -> KResult<()> {
    let cap_len = regs::read_caplength(base) as usize;
    let obase = base + cap_len;
    let hci_ver = regs::read_hciversion(base);
    let sparams1 = regs::read_hcsparams1(base);
    let max_slots = (sparams1 & 0xFF) as u8;
    let max_intrs = ((sparams1 >> 8) & 0xFF) as u8;
    let max_ports = ((sparams1 >> 16) & 0xFF) as u8;
    let hccparams1 = regs::read_hccparams1(base);
    let dboff = regs::read_dboff(base) as usize;
    let rtsoff = regs::read_rtsoff(base) as usize;
    let page_size = regs::op_r32(obase, regs::OP_PAGESIZE);
    G_XHCI = XhciCtx {
        base,
        obase,
        dboff,
        rtsoff,
        cap_len: cap_len as u8,
        hci_version: hci_ver,
        max_slots,
        max_intrs,
        max_ports,
        page_size,
        dcbaap: ptr::null_mut(),
        cmd_ring: ring::TrbRing {
            base: ptr::null_mut(),
            phys: 0,
            size: 0,
            enqueue: 0,
            cycle: false,
        },
        event_ring: ring::EventRing {
            base: ptr::null_mut(),
            phys: 0,
            size: 0,
            dequeue: 0,
            cycle: false,
        },
        xfer_rings: [ptr::null_mut(); 32],
        slot: 0,
        operational: false,
    };
    regs::op_w32(obase, regs::OP_USBCMD, regs::CMD_HCRST);
    let mut timeout = 100_000u32;
    while timeout > 0 && (regs::op_r32(obase, regs::OP_USBSTS) & regs::STS_CNR) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        return Err(Errno::Io);
    }
    regs::op_w32(obase, regs::OP_CONFIG, max_slots as u32);
    let dcbaap_pa = pmm::alloc_zero()? as *mut u64;
    ptr::write_bytes(dcbaap_pa, 0, (max_slots as usize + 1) * 8);
    G_XHCI.dcbaap = dcbaap_pa;
    regs::op_w64(obase, regs::OP_DCBAAP, dcbaap_pa as u64);
    if (hccparams1 & regs::HCC_SPS) != 0 {
        let sp_bufs = ((sparams1 >> 16) & 0x1F) as usize;
        for i in 0..sp_bufs {
            let sp = pmm::alloc_zero()? as *mut u8;
            ptr::write_bytes(sp, 0, 4096);
            let arr = dcbaap_pa;
            ptr::write(arr.add(1 + i), sp as u64);
        }
    }
    let cmd_ring = ring::alloc_ring(32)?;
    let cmd_phys = cmd_ring.phys;
    G_XHCI.cmd_ring = cmd_ring;
    regs::op_w64(
        obase,
        regs::OP_CRCR,
        cmd_phys | (regs::CRCR_ENABLE as u64) | (regs::CRCR_RCS as u64),
    );
    let event_ring = ring::alloc_event_ring(32)?;
    let er_phys = event_ring.phys;
    G_XHCI.event_ring = event_ring;
    regs::rt_w64(rtsoff, 0, regs::RTS_ERSTBA, er_phys);
    regs::rt_w32(rtsoff, 0, regs::RTS_ERSTSZ, 1);
    regs::rt_w64(rtsoff, 0, regs::RTS_ERDP, er_phys);
    regs::rt_w32(rtsoff, 0, regs::RTS_IMAN, regs::IMAN_IE);
    regs::op_w32(
        obase,
        regs::OP_USBCMD,
        regs::CMD_RUN | regs::CMD_INTES | regs::CMD_HSEE,
    );
    timeout = 100_000u32;
    while timeout > 0 && (regs::op_r32(obase, regs::OP_USBSTS) & regs::STS_HCHALTED) != 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        return Err(Errno::Io);
    }
    G_XHCI.operational = true;
    Ok(())
}
