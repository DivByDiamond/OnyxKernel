//! USB descriptor parsers for device and configuration blobs.
//!
//! Wire bytes are untrusted: every read is bounds-checked, every failure
//! is a `DescError`, and nothing here may panic on hostile input.

/// bDescriptorType: standard device descriptor.
pub const DESC_DEVICE: u8 = 0x01;
/// bDescriptorType: standard configuration descriptor.
pub const DESC_CONFIGURATION: u8 = 0x02;
/// bDescriptorType: standard interface descriptor.
pub const DESC_INTERFACE: u8 = 0x04;
/// bDescriptorType: standard endpoint descriptor.
pub const DESC_ENDPOINT: u8 = 0x05;
/// bInterfaceClass: HID.
pub const CLASS_HID: u8 = 0x03;

const DEVICE_DESC_LEN: usize = 18;
const CONFIG_HEADER_LEN: usize = 9;
const IFACE_DESC_LEN: usize = 9;
const ENDPOINT_DESC_LEN: usize = 7;
/// Capacity of the bounded arrays in `ConfigDesc`/`IfaceDesc`.
const MAX_IFACES: usize = 2;
const MAX_EPS_PER_IFACE: usize = 4;

/// Parsed 18-byte standard device descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DeviceDesc {
    pub max_packet_size0: u16,
    pub vendor: u16,
    pub product: u16,
    pub n_configs: u8,
}

/// One parsed standard endpoint descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EndpointDesc {
    pub address: u8,
    pub attributes: u8,
    pub max_packet: u16,
    pub interval: u8,
}

/// One parsed standard interface with up to `MAX_EPS_PER_IFACE` endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IfaceDesc {
    pub class: u8,
    pub protocol: u8,
    pub endpoints: [EndpointDesc; MAX_EPS_PER_IFACE],
    pub n_endpoints: u8,
}

/// One parsed standard configuration with up to `MAX_IFACES` interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ConfigDesc {
    pub interfaces: [IfaceDesc; MAX_IFACES],
    pub n_ifaces: u8,
}

/// Why a descriptor blob was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescError {
    /// The buffer (or the claimed wTotalLength) is shorter than the
    /// structure being parsed requires.
    Short,
    /// bDescriptorType does not match what its position requires; carries
    /// the offending byte (an ENDPOINT before any INTERFACE counts too).
    BadType(u8),
    /// A length field is malformed or a bounded array is full; carries the
    /// offending value (bad bLength, or the overflowing count).
    BadLen(u8),
}

/// Read a little-endian u16 at `off`; the caller has validated that `b`
/// covers `off + 2`.
fn le16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

/// Parse an 18-byte standard device descriptor.
pub fn parse_device(b: &[u8]) -> Result<DeviceDesc, DescError> {
    if b.len() < DEVICE_DESC_LEN {
        return Err(DescError::Short);
    }
    if (b[0] as usize) < DEVICE_DESC_LEN {
        // The descriptor claims fewer bytes than its own fixed layout.
        return Err(DescError::BadLen(b[0]));
    }
    if b[1] != DESC_DEVICE {
        return Err(DescError::BadType(b[1]));
    }
    Ok(DeviceDesc {
        max_packet_size0: b[7] as u16,
        vendor: le16(b, 8),
        product: le16(b, 10),
        n_configs: b[17],
    })
}

/// Parse a configuration blob: a 9-byte header followed by a chain of
/// bLength-delimited sub-descriptors. Interfaces and endpoints land in
/// the bounded arrays; every other descriptor type (HID, string,
/// class-specific) is skipped by its bLength. `n_ifaces`/`n_endpoints`
/// count what was actually parsed, not the header's claims.
pub fn parse_config(b: &[u8]) -> Result<ConfigDesc, DescError> {
    if b.len() < CONFIG_HEADER_LEN {
        return Err(DescError::Short);
    }
    if b[1] != DESC_CONFIGURATION {
        return Err(DescError::BadType(b[1]));
    }
    let total_len = le16(b, 2) as usize;
    if total_len < CONFIG_HEADER_LEN {
        // A config shorter than its own header cannot hold anything.
        return Err(DescError::Short);
    }
    if total_len > b.len() {
        // The blob is truncated relative to the claimed wTotalLength.
        return Err(DescError::Short);
    }
    let mut cfg = ConfigDesc::default();
    let mut cur_iface: Option<usize> = None;
    let mut i = CONFIG_HEADER_LEN;
    while i < total_len {
        // Proven: i < total_len; kept overflow-proof against hostile bytes.
        let remaining = total_len.checked_sub(i).ok_or(DescError::Short)?;
        if remaining < 2 {
            // Not even bLength + bDescriptorType fit.
            return Err(DescError::Short);
        }
        let dlen = b[i] as usize;
        let dtype = b[i + 1];
        if dlen < 2 || dlen > remaining {
            // bLength 0/1 would stall the walk; past remaining it is malformed.
            return Err(DescError::BadLen(dlen as u8));
        }
        match dtype {
            DESC_INTERFACE => add_iface(&mut cfg, &mut cur_iface, &b[i..i + dlen])?,
            DESC_ENDPOINT => add_endpoint(&mut cfg, cur_iface, &b[i..i + dlen])?,
            _ => {
                // Anything else (HID 0x21, string, class-specific) is
                // skipped by its bLength.
            }
        }
        // dlen >= 2 is enforced above, so the walk always advances.
        i += dlen;
    }
    Ok(cfg)
}

/// Start a new interface from an INTERFACE descriptor body.
fn add_iface(cfg: &mut ConfigDesc, cur: &mut Option<usize>, d: &[u8]) -> Result<(), DescError> {
    if d.len() < IFACE_DESC_LEN {
        return Err(DescError::BadLen(d.len() as u8));
    }
    if cfg.n_ifaces as usize >= MAX_IFACES {
        // A third interface does not fit the bounded array.
        return Err(DescError::BadLen(MAX_IFACES as u8 + 1));
    }
    let idx = cfg.n_ifaces as usize;
    cfg.interfaces[idx] = IfaceDesc {
        class: d[5],
        protocol: d[7],
        ..IfaceDesc::default()
    };
    cfg.n_ifaces += 1;
    *cur = Some(idx);
    Ok(())
}

/// Attach an ENDPOINT descriptor body to the most recent interface.
fn add_endpoint(cfg: &mut ConfigDesc, cur: Option<usize>, d: &[u8]) -> Result<(), DescError> {
    if d.len() < ENDPOINT_DESC_LEN {
        return Err(DescError::BadLen(d.len() as u8));
    }
    // An endpoint before any interface has no owner — chain-order violation.
    let idx = cur.ok_or(DescError::BadType(DESC_ENDPOINT))?;
    let iface = &mut cfg.interfaces[idx];
    if iface.n_endpoints as usize >= MAX_EPS_PER_IFACE {
        // A fifth endpoint does not fit the bounded array.
        return Err(DescError::BadLen(MAX_EPS_PER_IFACE as u8 + 1));
    }
    iface.endpoints[iface.n_endpoints as usize] = EndpointDesc {
        address: d[2],
        attributes: d[3],
        max_packet: le16(d, 4),
        interval: d[6],
    };
    iface.n_endpoints += 1;
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
