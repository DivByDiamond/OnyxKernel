//! The USB enumeration sequence over any [`ControlBus`]: device-
//! descriptor head read at the default address, SET_ADDRESS(1) with the
//! bus re-targeting, full descriptor reads, configuration-blob parse,
//! first-HID-interface selection and SET_CONFIGURATION.

use onyx_core::errno::{Errno, KResult};

use super::desc::{
    CLASS_HID, DESC_CONFIGURATION, DESC_DEVICE, DescError, EndpointDesc, parse_config, parse_device,
};
use super::requests::{ControlBus, get_descriptor, set_address, set_configuration};
use super::{Owner, UsbSlot};

/// Device address assigned by the enumeration pass — the first free
/// bus address after the default 0.
const ENUM_ADDR: u8 = 1;

/// bConfigurationValue selected after the parse: the first
/// configuration, as reported by every device this layer targets.
const CONFIG_NUM: u8 = 1;

/// First device-descriptor read length: just past bMaxPacketSize0 at
/// offset 7 (the classic two-step read — the full 18 bytes may exceed
/// the unknown EP0 max packet before the device is addressed).
const DEV_HEAD_LEN: u16 = 8;

/// Full standard device-descriptor length.
const DEV_DESC_LEN: u16 = 18;

/// Configuration-descriptor header length, through wTotalLength.
const CFG_HEADER_LEN: u16 = 9;

/// Configuration blob buffer capacity; also the cap on supported
/// wTotalLength — a config claiming more fails the parser's
/// truncation check, keeping hostile lengths out.
const CFG_BLOB_MAX: usize = 256;

/// bmAttributes bits 1:0 select the endpoint transfer type.
const EP_ATTR_TYPE_MASK: u8 = 0x03;
/// Interrupt transfer type in bmAttributes bits 1:0.
const EP_ATTR_INTERRUPT: u8 = 0x03;
/// Endpoint-address bit 7: IN direction (device → host).
const EP_DIR_IN: u8 = 0x80;

/// Legal EP0 max packet sizes (USB 2.0 §9.6.6); everything else in a
/// hostile head read is rejected before the device is addressed.
fn legal_mps0(mps0: u16) -> bool {
    matches!(mps0, 8 | 16 | 32 | 64)
}

/// Map a descriptor parser failure to its errno: truncated data is a
/// bus-level `Io`, structural violations are `Inval` (the response is
/// malformed, not merely missing).
fn map_desc_err(e: DescError) -> Errno {
    match e {
        DescError::Short => Errno::Io,
        DescError::BadType(_) | DescError::BadLen(_) => Errno::Inval,
    }
}

/// Run the enumeration sequence over `bus`: read the 8-byte device
/// descriptor head at the default address 0 and take bMaxPacketSize0
/// from it, issue SET_ADDRESS(1) and re-target the bus, read the full
/// 18-byte device descriptor, read the 9-byte configuration header for
/// wTotalLength, read the full configuration blob (256-byte buffer,
/// truncated to wTotalLength) and parse it, then select the first
/// interface with `class == CLASS_HID` and an interrupt-IN endpoint
/// (`attributes & 0x03 == 0x03`, `address & 0x80 != 0`) and issue
/// SET_CONFIGURATION(1).
///
/// Returns the filled slot with `addr = 1` on success; the caller
/// registers it via [`super::set_slot`]. Returns
/// `Err(Errno::NoEnt)` when the parsed configuration has no interface
/// with `class == CLASS_HID` and an interrupt-IN endpoint, and
/// `Err(Errno::Io)`/`Err(Errno::Inval)` for bus failures or malformed
/// (truncated or structurally invalid) responses. A failure mid-sequence
/// takes no slot: this function never touches the registry.
pub fn enumerate(bus: &mut dyn ControlBus, owner: Owner) -> KResult<UsbSlot> {
    // Step 1: descriptor head at address 0 — validate the type byte and
    // bMaxPacketSize0 before anything else moves.
    let mut head = [0u8; DEV_HEAD_LEN as usize];
    let n = bus.control(
        &get_descriptor(DESC_DEVICE, 0, DEV_HEAD_LEN),
        Some(&mut head),
        true,
    )?;
    if (n as usize) < head.len() {
        return Err(Errno::Io);
    }
    if head[1] != DESC_DEVICE {
        return Err(Errno::Inval);
    }
    let mps0 = head[7] as u16;
    if !legal_mps0(mps0) {
        return Err(Errno::Inval);
    }

    // Step 2: address the device; every later transfer goes through a
    // bus targeted at the new address (ruling 3).
    bus.control(&set_address(ENUM_ADDR), None, false)?;
    bus.set_addr(ENUM_ADDR);

    // Step 3: the full device descriptor — vendor/product identity.
    let mut dev_raw = [0u8; DEV_DESC_LEN as usize];
    let n = bus.control(
        &get_descriptor(DESC_DEVICE, 0, DEV_DESC_LEN),
        Some(&mut dev_raw),
        true,
    )?;
    if (n as usize) < dev_raw.len() {
        return Err(Errno::Io);
    }
    let dev = parse_device(&dev_raw).map_err(map_desc_err)?;

    // Step 4: configuration header for wTotalLength, then the full blob
    // truncated to it (capped at the buffer size).
    let mut hdr = [0u8; CFG_HEADER_LEN as usize];
    let n = bus.control(
        &get_descriptor(DESC_CONFIGURATION, 0, CFG_HEADER_LEN),
        Some(&mut hdr),
        true,
    )?;
    if (n as usize) < hdr.len() {
        return Err(Errno::Io);
    }
    let total_len = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
    if total_len < CFG_HEADER_LEN as usize {
        return Err(Errno::Inval);
    }
    let want = total_len.min(CFG_BLOB_MAX);
    let mut blob = [0u8; CFG_BLOB_MAX];
    let n = bus.control(
        &get_descriptor(DESC_CONFIGURATION, 0, want as u16),
        Some(&mut blob),
        true,
    )?;
    if (n as usize) < want {
        return Err(Errno::Io);
    }
    let cfg = parse_config(&blob[..want]).map_err(map_desc_err)?;

    // Step 5: first HID interface with an interrupt-IN endpoint.
    let mut picked: Option<(u8, EndpointDesc)> = None;
    for (i, iface) in cfg
        .interfaces
        .iter()
        .take(cfg.n_ifaces as usize)
        .enumerate()
    {
        if iface.class != CLASS_HID {
            continue;
        }
        for ep in iface.endpoints.iter().take(iface.n_endpoints as usize) {
            if ep.attributes & EP_ATTR_TYPE_MASK == EP_ATTR_INTERRUPT && ep.address & EP_DIR_IN != 0
            {
                picked = Some((i as u8, *ep));
                break;
            }
        }
        if picked.is_some() {
            break;
        }
    }
    let Some((iface_idx, ep)) = picked else {
        return Err(Errno::NoEnt);
    };

    // Step 6: select the configuration — the device is live afterwards.
    bus.control(&set_configuration(CONFIG_NUM), None, false)?;

    Ok(UsbSlot {
        active: true,
        owner,
        addr: ENUM_ADDR,
        vendor: dev.vendor,
        product: dev.product,
        class: CLASS_HID,
        iface: iface_idx,
        ep_in: ep.address,
        max_packet: ep.max_packet,
        interval: ep.interval,
        hc_slot: bus.hc_slot(),
    })
}
