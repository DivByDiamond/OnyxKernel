use onyx_core::errno::{Errno, KResult};

use super::core::{Urb, UrbStatus, UrbTransferType, UsbHcd};

pub struct EhciHcd;

impl EhciHcd {
    pub unsafe fn new() -> Self {
        EhciHcd
    }
}

impl UsbHcd for EhciHcd {
    fn submit_urb(&mut self, urb: &mut Urb) -> KResult<()> {
        match urb.transfer_type {
            UrbTransferType::Control => {
                let setup = urb.setup_packet.ok_or(Errno::Inval)?;
                let data_in = (setup[0] & 0x80) != 0;
                let max_pkt = urb.buffer_length.max(8);
                let n = unsafe {
                    crate::drivers::usb::ehci::ehci_control_transfer(
                        urb.dev_addr,
                        setup,
                        urb.buffer.as_deref_mut(),
                        data_in,
                        max_pkt,
                    )?
                };
                urb.actual_length = n;
                urb.status = UrbStatus::NoError;
                if let Some(cb) = urb.complete {
                    cb(urb as *mut Urb);
                }
                Ok(())
            }
            UrbTransferType::Bulk => {
                let data_in = (urb.endpoint & 0x80) != 0;
                let n = unsafe {
                    crate::drivers::usb::ehci::ehci_bulk_transfer(
                        urb.dev_addr,
                        urb.buffer.as_deref_mut(),
                        data_in,
                        512,
                    )?
                };
                urb.actual_length = n;
                urb.status = UrbStatus::NoError;
                if let Some(cb) = urb.complete {
                    cb(urb as *mut Urb);
                }
                Ok(())
            }
            _ => Err(Errno::NoSys),
        }
    }

    fn cancel_urb(&mut self, _urb: &mut Urb) -> KResult<()> {
        Err(Errno::NoSys)
    }

    fn port_reset(&mut self, port: u8) -> KResult<()> {
        unsafe { crate::drivers::usb::port_reset(port) }
    }

    fn port_enable(&mut self, port: u8) -> KResult<()> {
        unsafe { crate::drivers::usb::port_enable(port) }
    }

    fn port_status(&mut self, port: u8) -> KResult<u32> {
        unsafe { crate::drivers::usb::port_status(port) }
    }

    fn n_ports(&self) -> u8 {
        crate::drivers::usb::n_ports()
    }
}

pub struct OhciHcd;

impl OhciHcd {
    pub unsafe fn new() -> Self {
        OhciHcd
    }
}

impl UsbHcd for OhciHcd {
    fn submit_urb(&mut self, urb: &mut Urb) -> KResult<()> {
        match urb.transfer_type {
            UrbTransferType::Control => {
                let setup = urb.setup_packet.ok_or(Errno::Inval)?;
                let data_in = (setup[0] & 0x80) != 0;
                let max_pkt = urb.buffer_length.max(8);
                let speed = unsafe { crate::drivers::usb::ohci::ohci_port_speed(0) }.unwrap_or(0);
                let n = unsafe {
                    crate::drivers::usb::ohci::ohci_control_transfer(
                        urb.dev_addr,
                        setup,
                        urb.buffer.as_deref_mut(),
                        data_in,
                        max_pkt,
                        speed,
                    )?
                };
                urb.actual_length = n;
                urb.status = UrbStatus::NoError;
                if let Some(cb) = urb.complete {
                    cb(urb as *mut Urb);
                }
                Ok(())
            }
            UrbTransferType::Bulk => {
                let data_in = (urb.endpoint & 0x80) != 0;
                let n = unsafe {
                    crate::drivers::usb::ohci::ohci_bulk_transfer(
                        urb.dev_addr,
                        urb.buffer.as_deref_mut(),
                        data_in,
                        512,
                        0,
                    )?
                };
                urb.actual_length = n;
                urb.status = UrbStatus::NoError;
                if let Some(cb) = urb.complete {
                    cb(urb as *mut Urb);
                }
                Ok(())
            }
            _ => Err(Errno::NoSys),
        }
    }

    fn cancel_urb(&mut self, _urb: &mut Urb) -> KResult<()> {
        Err(Errno::NoSys)
    }

    fn port_reset(&mut self, port: u8) -> KResult<()> {
        unsafe { crate::drivers::usb::ohci::ohci_port_reset(port) }
    }

    fn port_enable(&mut self, port: u8) -> KResult<()> {
        unsafe { crate::drivers::usb::ohci::ohci_port_enable(port) }
    }

    fn port_status(&mut self, port: u8) -> KResult<u32> {
        unsafe { crate::drivers::usb::ohci::ohci_port_status(port) }
    }

    fn n_ports(&self) -> u8 {
        unsafe { crate::drivers::usb::ohci::ohci_n_ports() }
    }
}
