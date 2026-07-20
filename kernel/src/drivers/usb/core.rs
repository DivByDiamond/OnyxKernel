//! URB (USB Request Block) abstraction layer.

use onyx_core::errno::KResult;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UrbStatus {
    NoError,
    Stalled,
    Babble,
    Timeout,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UrbTransferType {
    Control,
    Bulk,
    Interrupt,
}

pub struct Urb {
    pub endpoint: u8,
    pub transfer_type: UrbTransferType,
    pub dev_addr: u8,
    pub setup_packet: Option<&'static [u8; 8]>,
    pub buffer: Option<&'static mut [u8]>,
    pub buffer_length: u32,
    pub actual_length: u32,
    pub status: UrbStatus,
    pub complete: Option<fn(*mut Urb)>,
    pub context: *mut core::ffi::c_void,
}

impl Urb {
    pub fn new_control(
        dev_addr: u8,
        setup: &'static [u8; 8],
        data: Option<&'static mut [u8]>,
    ) -> Self {
        let len = data.as_ref().map(|d| d.len() as u32).unwrap_or(0);
        Urb {
            endpoint: 0,
            transfer_type: UrbTransferType::Control,
            dev_addr,
            setup_packet: Some(setup),
            buffer: data,
            buffer_length: len,
            actual_length: 0,
            status: UrbStatus::NoError,
            complete: None,
            context: core::ptr::null_mut(),
        }
    }

    pub fn new_bulk(dev_addr: u8, endpoint: u8, data: &'static mut [u8]) -> Self {
        let len = data.len() as u32;
        Urb {
            endpoint,
            transfer_type: UrbTransferType::Bulk,
            dev_addr,
            setup_packet: None,
            buffer: Some(data),
            buffer_length: len,
            actual_length: 0,
            status: UrbStatus::NoError,
            complete: None,
            context: core::ptr::null_mut(),
        }
    }
}

pub struct UsbDevice {
    pub address: u8,
    pub speed: u8,
    pub max_packet_size0: u16,
    pub descriptors: &'static [u8],
}

pub trait UsbHcd {
    fn submit_urb(&mut self, urb: &mut Urb) -> KResult<()>;
    fn cancel_urb(&mut self, urb: &mut Urb) -> KResult<()>;
    fn port_reset(&mut self, port: u8) -> KResult<()>;
    fn port_enable(&mut self, port: u8) -> KResult<()>;
    fn port_status(&mut self, port: u8) -> KResult<u32>;
    fn n_ports(&self) -> u8;
}
