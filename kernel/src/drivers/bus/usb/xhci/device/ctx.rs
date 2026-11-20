//! xHCI device-context layouts: slot, endpoint and input-control contexts.
//!
//! Endpoint context field positions per §6.2.3 Tables 6-8..6-11: DW0 bits
//! [23:16] Interval, DW1 bits [31:16] Max Packet Size / [7:5] EP Type /
//! [2:1] CErr, DW2/DW3 the 64-bit TRB Dequeue Pointer with DCS at DW2 bit
//! 0, DW4 bits [15:0] Average TRB Length and [31:16] Max ESIT Payload.

/// CErr field value (DW1 bits [2:1]): re-try a transfer three times
/// before reporting an error (§6.2.3, same value Linux programs).
pub(crate) const CERR: u32 = 3;

#[repr(C)]
pub(crate) struct SlotCtx {
    pub(crate) dw0: u32,
    pub(crate) dw1: u32,
    pub(crate) dw2: u32,
    pub(crate) dw3: u32,
}

#[repr(C)]
pub(crate) struct EpCtx {
    pub(crate) dw0: u32,
    pub(crate) dw1: u32,
    pub(crate) dw2: u32,
    pub(crate) dw3: u32,
    pub(crate) dw4: u32,
}

#[repr(C)]
pub(crate) struct InputCtx {
    pub(crate) drop_flags: u32,
    pub(crate) add_flags: u32,
    pub(crate) rsvd: [u32; 6],
}

/// Program the slot context for a root-hub-attached device.
///
/// `port` is the 0-based root-port index: per xHCI §6.2.2 Table 6-4 the
/// Route String in DW0 bits [19:0] stays zero for a device attached
/// directly to a root hub port (the root port is NOT part of the route
/// string), and the 1-based Root Hub Port Number goes into DW1 bits
/// [23:16] — ports are numbered 1..MaxPorts, and QEMU's
/// xhci_lookup_uport rejects a slot whose DW1 port field is 0.
///
/// DW0 also carries the speed at bits [23:20] and Context Entries
/// (`entries`: the highest valid DCI — 1 after Address Device, more once
/// non-EP0 endpoints are configured) at bits [31:27].
pub(crate) fn slot_ctx_root_port(sc: &mut SlotCtx, port: u8, speed: u8, entries: u8) {
    sc.dw0 = (speed as u32) << 20 | (entries as u32) << 27;
    sc.dw1 = (u32::from(port) + 1) << 16;
    sc.dw2 = 0;
    sc.dw3 = 0;
}
