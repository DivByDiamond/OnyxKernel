//! Host tests for the context field encodings programmed for the
//! Address Device command: the slot context (§6.2.2 Table 6-4 — Route
//! String in DW0 bits [19:0] is zero for a device attached directly to a
//! root hub port, the 1-based Root Hub Port Number sits in DW1 bits
//! [23:16]) and the EP0 context (§6.2.3 Tables 6-8..6-11 — Interval in
//! DW0 bits [23:16], Max Packet Size/EP Type/CErr in DW1, the 64-bit TRB
//! Dequeue Pointer with DCS in DW2/DW3, Average TRB Length in DW4).

use super::*;

/// 32 dwords: covers the input-control context, the slot context and the
/// EP0 context (offsets 0/32/64 bytes) with room to spare.
const CTX_DWS: usize = 32;

/// A zeroed, u32-aligned context buffer (both helpers write u32 fields
/// through raw pointers, so the backing store must be u32-aligned).
fn ctx_buf() -> [u32; CTX_DWS] {
    [0u32; CTX_DWS]
}

#[test]
fn input_ctx_slot_matches_slot_ctx_encoding() {
    let mut buf = ctx_buf();
    // SAFETY: local u32-aligned buffer covering offsets 0..=96 (input
    // control + slot + EP0 contexts); exclusive stack access. No EP0
    // ring on the host: dequeue 0, DCS clear.
    unsafe { set_input_ctx_slot(buf.as_mut_ptr() as *mut u8, 0, SPEED_FS, 8, 0, false) };
    // Input control context: add slot + EP0 (flags 3), nothing dropped.
    assert_eq!(buf[0], 0);
    assert_eq!(buf[1], 3);
    // Slot context at dword offset 8: same encoding as
    // ctx::slot_ctx_root_port — route string zero, port index 0 →
    // root hub port 1 in DW1 [23:16].
    assert_eq!(buf[8], (SPEED_FS as u32) << 20 | 1 << 27);
    assert_eq!(buf[9], 1 << 16);
    // EP0 at dword offset 16: DW0 Interval [23:16] is 0 for a control EP.
    assert_eq!(buf[16], 0);
    // DW1: mps0=8 in bits [31:16], EP Type=Control in [7:5], CErr=3 [2:1].
    assert_eq!(buf[17], 8 << 16 | (EP_TYPE_CONTROL as u32) << 3 | 3 << 1);
    // DW2/DW3: dequeue + DCS (zero on the host: no EP0 ring published).
    assert_eq!(buf[18], 0);
    assert_eq!(buf[19], 0);
    // DW4: Average TRB Length 8.
    assert_eq!(buf[20], 8);
}

#[test]
fn input_ctx_programs_the_devices_own_ep0_ring() {
    // Per-device EP0 rings: the dequeue pointer and DCS come from the
    // caller's ring, not a shared global.
    let mut buf = ctx_buf();
    // SAFETY: as above.
    unsafe {
        set_input_ctx_slot(
            buf.as_mut_ptr() as *mut u8,
            5,
            SPEED_HS,
            64,
            0x1_8000_2000,
            true,
        )
    };
    assert_eq!(buf[9], 6 << 16); // root port index 5 → port 6
    assert_eq!(buf[18], 0x8000_2001); // dequeue low | DCS
    assert_eq!(buf[19], 1); // dequeue high
}
