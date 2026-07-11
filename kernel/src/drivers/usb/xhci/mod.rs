use crate::arch::mmio::Mmio;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub mod device;
pub mod mass;
pub mod regs;
pub mod ring;

pub struct XhciCtx {
    pub base: usize,
    pub obase: usize,
    pub dboff: usize,
    pub rtsoff: usize,
    pub cap_len: u8,
    pub hci_version: u16,
    pub max_slots: u8,
    pub max_intrs: u8,
    pub max_ports: u8,
    pub page_size: u32,
    pub dcbaap: *mut u64,
    pub cmd_ring: ring::TrbRing,
    pub event_ring: ring::EventRing,
    pub xfer_rings: [*mut ring::TrbRing; 32],
    pub slot: u8,
    pub operational: bool,
}

pub(crate) static mut G_XHCI: XhciCtx = XhciCtx {
    base: 0,
    obase: 0,
    dboff: 0,
    rtsoff: 0,
    cap_len: 0,
    hci_version: 0,
    max_slots: 0,
    max_intrs: 0,
    max_ports: 0,
    page_size: 0,
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

pub unsafe fn probe(base: usize) -> bool {
    let hci_ver = regs::read_hciversion(base);
    hci_ver >= 0x100
}

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

pub unsafe fn port_connect(port: u8) -> bool {
    let reg = regs::OP_PORTSC + (port as u32) * 0x10;
    let v = regs::op_r32(G_XHCI.obase, reg);
    (v & regs::PORT_CCS) != 0
}

pub unsafe fn enable_slot() -> KResult<u8> {
    let mut trb = ring::Trb::zero();
    trb.set_type(ring::TRB_ENABLE_SLOT);
    trb.set_flags(ring::TRB_IOC);
    let ev = ring::submit_command(&trb)?;
    let slot_id = (ev.params[3] >> 24) as u8;
    if slot_id == 0 {
        return Err(Errno::Io);
    }
    G_XHCI.slot = slot_id;
    Ok(slot_id)
}

pub unsafe fn address_device(slot_id: u8, input_ctx_pa: u64) -> KResult<()> {
    let mut trb = ring::Trb::zero();
    trb.params[0] = input_ctx_pa as u32;
    trb.params[1] = (input_ctx_pa >> 32) as u32;
    trb.params[2] = (slot_id as u32) << 24;
    trb.set_type(ring::TRB_ADDRESS_DEVICE);
    trb.set_flags(ring::TRB_IOC);
    ring::submit_command(&trb)?;
    let dcbaap = G_XHCI.dcbaap;
    let dev_ctx_pa = ptr::read(dcbaap.add(slot_id as usize));
    if dev_ctx_pa == 0 {
        return Err(Errno::Io);
    }
    Ok(())
}

pub unsafe fn irq_handler() {
    let ctx = &raw const G_XHCI;
    if !(*ctx).operational {
        return;
    }
    let iman = regs::rt_r32((*ctx).rtsoff, 0, regs::RTS_IMAN);
    if (iman & regs::IMAN_IP) != 0 {
        regs::rt_w32((*ctx).rtsoff, 0, regs::RTS_IMAN, regs::IMAN_IP);
    }
}
