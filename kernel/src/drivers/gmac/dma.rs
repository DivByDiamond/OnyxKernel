use super::regs;
use super::{G_GMAC, RX_RING_SIZE, TX_RING_SIZE};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct DmaDesc {
    pub buf_addr: u32,
    pub flags: u32,
}

static mut TX_DESC_RING: *mut DmaDesc = ptr::null_mut();
static mut RX_DESC_RING: *mut DmaDesc = ptr::null_mut();
static mut TX_BUFS: [*mut u8; 16] = [ptr::null_mut(); 16];
static mut RX_BUFS: [*mut u8; 16] = [ptr::null_mut(); 16];

pub unsafe fn init_tx_rings() -> KResult<usize> {
    let desc_pa = pmm::alloc_zero()? as usize;
    TX_DESC_RING = desc_pa as *mut DmaDesc;
    for i in 0..TX_RING_SIZE {
        let buf_pa = pmm::alloc_zero()? as *mut u8;
        TX_BUFS[i as usize] = buf_pa;
        let desc = &raw mut *TX_DESC_RING.offset(i as isize);
        let ter = if i == TX_RING_SIZE - 1 {
            regs::TDES1_TER
        } else {
            0
        };
        ptr::write_volatile(ptr::addr_of_mut!((*desc).buf_addr), buf_pa as u32);
        ptr::write_volatile(ptr::addr_of_mut!((*desc).flags), ter | regs::TDES1_TCH);
    }
    regs::reg_w(G_GMAC.base, regs::DMA_TX_BASE_ADDR, desc_pa as u32);
    Ok(desc_pa)
}

pub unsafe fn init_rx_rings() -> KResult<usize> {
    let desc_pa = pmm::alloc_zero()? as usize;
    RX_DESC_RING = desc_pa as *mut DmaDesc;
    for i in 0..RX_RING_SIZE {
        let buf_pa = pmm::alloc_zero()? as *mut u8;
        RX_BUFS[i as usize] = buf_pa;
        let desc = &raw mut *RX_DESC_RING.offset(i as isize);
        let ter = if i == RX_RING_SIZE - 1 {
            regs::RDES1_TER
        } else {
            0
        };
        ptr::write_volatile(ptr::addr_of_mut!((*desc).buf_addr), buf_pa as u32);
        ptr::write_volatile(
            ptr::addr_of_mut!((*desc).flags),
            regs::RDES1_OWN
                | ter
                | regs::RDES1_RCH
                | (super::GMAC_BUF_SIZE as u32 & regs::RDES1_BS1_MASK),
        );
    }
    regs::reg_w(G_GMAC.base, regs::DMA_RX_BASE_ADDR, desc_pa as u32);
    Ok(desc_pa)
}

pub unsafe fn tx_desc_vaddr(idx: u16) -> *mut DmaDesc {
    TX_DESC_RING.offset(idx as isize)
}

pub unsafe fn rx_desc_vaddr(idx: u16) -> *mut DmaDesc {
    RX_DESC_RING.offset(idx as isize)
}

pub unsafe fn tx_buf_vaddr(idx: u16) -> *mut u8 {
    TX_BUFS[idx as usize]
}

pub unsafe fn rx_buf_vaddr(idx: u16) -> *mut u8 {
    RX_BUFS[idx as usize]
}
