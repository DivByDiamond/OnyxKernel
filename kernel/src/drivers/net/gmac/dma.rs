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

/// # Safety
///
/// Must run once during single-threaded GMAC init (SIE=0) before the DMA
/// engine is started; allocates and fills the TX descriptor ring.
pub unsafe fn init_tx_rings() -> KResult<usize> {
    // SAFETY: TX_DESC_RING/TX_BUFS are written only here during single-threaded init (SIE=0); pages come from pmm::alloc_zero (physically contiguous) and volatile stores stay within each 4 KiB page (16 x 8-byte descriptors, one buffer page per entry).
    unsafe {
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
}

/// # Safety
///
/// Must run once during single-threaded GMAC init (SIE=0) before the DMA
/// engine is started; allocates and fills the RX descriptor ring.
pub unsafe fn init_rx_rings() -> KResult<usize> {
    // SAFETY: RX_DESC_RING/RX_BUFS are written only here during single-threaded init (SIE=0); pages come from pmm::alloc_zero (physically contiguous) and volatile stores stay within each 4 KiB page.
    unsafe {
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
}

/// # Safety
///
/// Caller contract: `init_tx_rings` must have completed and `idx` must be
/// < TX_RING_SIZE so the offset stays inside the descriptor page.
pub unsafe fn tx_desc_vaddr(idx: u16) -> *mut DmaDesc {
    // SAFETY: TX_DESC_RING was set by init_tx_rings; idx < TX_RING_SIZE keeps TX_DESC_RING.offset(idx) within the allocated descriptor page.
    unsafe { TX_DESC_RING.offset(idx as isize) }
}

/// # Safety
///
/// Caller contract: `init_rx_rings` must have completed and `idx` must be
/// < RX_RING_SIZE so the offset stays inside the descriptor page.
pub unsafe fn rx_desc_vaddr(idx: u16) -> *mut DmaDesc {
    // SAFETY: RX_DESC_RING was set by init_rx_rings; idx < RX_RING_SIZE keeps RX_DESC_RING.offset(idx) within the allocated descriptor page.
    unsafe { RX_DESC_RING.offset(idx as isize) }
}

/// # Safety
///
/// Caller contract: `init_tx_rings` must have completed and `idx` must be
/// < TX_RING_SIZE (== TX_BUFS length).
pub unsafe fn tx_buf_vaddr(idx: u16) -> *mut u8 {
    // SAFETY: static-mut read of TX_BUFS with idx < 16 (== array length, bounds-checked), single-threaded context (SIE=0); pointer targets the buffer page allocated by init_tx_rings.
    unsafe { TX_BUFS[idx as usize] }
}

/// # Safety
///
/// Caller contract: `init_rx_rings` must have completed and `idx` must be
/// < RX_RING_SIZE (== RX_BUFS length).
pub unsafe fn rx_buf_vaddr(idx: u16) -> *mut u8 {
    // SAFETY: static-mut read of RX_BUFS with idx < 16 (== array length, bounds-checked), single-threaded context (SIE=0); pointer targets the buffer page allocated by init_rx_rings.
    unsafe { RX_BUFS[idx as usize] }
}
