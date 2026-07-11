use super::dma::{rx_buf_vaddr, rx_desc_vaddr, tx_buf_vaddr, tx_desc_vaddr};
use super::regs;
use super::{G_GMAC, GMAC_BUF_SIZE, TX_RING_SIZE};
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn send(data: &[u8]) -> KResult<()> {
    if data.is_empty() || data.len() > GMAC_BUF_SIZE {
        return Err(Errno::Inval);
    }
    let idx = G_GMAC.tx_cur;
    let desc = tx_desc_vaddr(idx);
    let buf = tx_buf_vaddr(idx);
    ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());
    let ter = if idx == TX_RING_SIZE - 1 {
        regs::TDES1_TER
    } else {
        0
    };
    let flags = regs::TDES1_OWN
        | ter
        | regs::TDES1_TCH
        | regs::TDES1_FS
        | regs::TDES1_LS
        | ((data.len() as u32) & regs::TDES1_BS1_MASK);
    ptr::write_volatile(ptr::addr_of_mut!((*desc).buf_addr), buf as u32);
    ptr::write_volatile(ptr::addr_of_mut!((*desc).flags), flags);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    regs::reg_w(G_GMAC.base, regs::DMA_XMT_POLL_DEMAND, 1);
    let mut t = 500_000u32;
    while t > 0 {
        let f = ptr::read_volatile(ptr::addr_of!((*desc).flags));
        if f & regs::TDES1_OWN == 0 {
            break;
        }
        t -= 1;
    }
    if t == 0 {
        return Err(Errno::Io);
    }
    G_GMAC.tx_cur = (idx + 1) % TX_RING_SIZE;
    Ok(())
}

pub unsafe fn recv_into(buf: &mut [u8]) -> KResult<usize> {
    let idx = G_GMAC.rx_cur;
    let desc = rx_desc_vaddr(idx);
    let f = ptr::read_volatile(ptr::addr_of!((*desc).flags));
    if f & regs::RDES1_OWN != 0 {
        return Err(Errno::NoEnt);
    }
    let rdes0 = ptr::read_volatile(ptr::addr_of!((*desc).buf_addr));
    let frame_len = ((rdes0 & regs::RDES0_FL_MASK) >> 16) as usize;
    let len = frame_len.min(buf.len());
    let src = rx_buf_vaddr(idx);
    ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), len);
    let ter = if idx == 15 { regs::RDES1_TER } else { 0 };
    let new_addr = src as u32;
    ptr::write_volatile(ptr::addr_of_mut!((*desc).buf_addr), new_addr);
    ptr::write_volatile(
        ptr::addr_of_mut!((*desc).flags),
        regs::RDES1_OWN | ter | regs::RDES1_RCH | (GMAC_BUF_SIZE as u32 & regs::RDES1_BS1_MASK),
    );
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    G_GMAC.rx_cur = (idx + 1) % 16;
    Ok(len)
}

pub unsafe fn get_mac(base: usize) -> [u8; 6] {
    let hi = regs::reg_r(base, regs::MAC_ADDR0_HI);
    let lo = regs::reg_r(base, regs::MAC_ADDR0_LO);
    [
        (hi >> 24) as u8,
        (hi >> 16) as u8,
        (hi >> 8) as u8,
        hi as u8,
        (lo >> 8) as u8,
        lo as u8,
    ]
}
