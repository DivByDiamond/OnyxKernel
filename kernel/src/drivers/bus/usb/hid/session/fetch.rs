//! The host-testable half of [`super::attach`]: the HID 1.11 §7.2.1
//! report-descriptor fetch over a `ControlBus`, and its parse. No
//! MMIO — the host tests drive this with a scripted bus.

use onyx_core::errno::{Errno, KResult};

use crate::drivers::usb::dev::requests::{ControlBus, REQ_GET_DESCRIPTOR, RTYPE_IFACE_IN, setup};

use super::diff::{SlotSession, apply_report};
use super::{HidEvent, MAX_PEND, MAX_REPORT};

use crate::drivers::usb::hid::{HID_DESC_REPORT, HidError, HidTable, has_axes, parse_report_desc};

/// Bounded report-descriptor read: the spec sets no length cap and
/// real sticks stay tiny (the T.16000M-class fixture ~40 bytes), so a
/// page-sized stack read covers everything with one control transfer;
/// a device with a shorter descriptor answers short and the returned
/// length is what gets parsed.
pub(crate) const DESC_READ: u16 = 512;

/// Build the HID 1.11 §7.2.1 report-descriptor request: GET_DESCRIPTOR
/// with bmRequestType 0x81 (IN, interface recipient), the report
/// descriptor type 0x22 in wValue's high byte, wIndex = `iface`.
fn report_desc_request(iface: u8, len: u16) -> [u8; 8] {
    let value = u16::from(HID_DESC_REPORT) << 8;
    setup(
        RTYPE_IFACE_IN,
        REQ_GET_DESCRIPTOR,
        value,
        u16::from(iface),
        len,
    )
}

/// HID 1.11 §7.2.1 class request GET_REPORT: bmRequestType 0xA1 (IN,
/// class, interface), bRequest 0x01, wValue = report type Input (1) in
/// the high byte with report id 0 (report ids are rejected at parse).
const RTYPE_CLASS_IFACE_IN: u8 = 0xA1;
const REQ_GET_REPORT: u8 = 0x01;
const INPUT_REPORT: u16 = 0x0100;

/// Build the session of `iface`: parse its report descriptor, then seed
/// the baseline with the device's current input report. Without the seed
/// a device that reports only on change (QEMU usb-tablet) has its first
/// real motion delivered as the `init` burst. A stalled or short
/// GET_REPORT is not an attach failure: the first polled report then
/// becomes the baseline.
pub(crate) fn fetch_session(bus: &mut dyn ControlBus, iface: u8) -> KResult<SlotSession> {
    let mut session = SlotSession::from_table(fetch_table(bus, iface)?);
    let pkt = setup(
        RTYPE_CLASS_IFACE_IN,
        REQ_GET_REPORT,
        INPUT_REPORT,
        u16::from(iface),
        MAX_REPORT as u16,
    );
    let mut report = [0u8; MAX_REPORT];
    if let Ok(n) = bus.control(&pkt, Some(&mut report), true) {
        let end = (n as usize).min(report.len());
        // The events are state, not motion: only the baseline is kept.
        let mut seed = [(HidEvent::Axis { axis: 0, value: 0 }, false); MAX_PEND];
        apply_report(&mut session, &report[..end], &mut seed);
    }
    Ok(session)
}

/// Map parse failures onto the attach error contract.
fn hid_err(e: HidError) -> Errno {
    match e {
        // Not a stick: the slot stays a plain enumerated device — the
        // caller logs info, not a warning (ruling 5).
        HidError::NotStick => Errno::NoEnt,
        // Short/UnknownItem/TooManyFields/ReportId: malformed or
        // unsupported descriptor.
        _ => Errno::Inval,
    }
}

/// Fetch the interface's report descriptor over `bus` and parse it.
/// One bounded read of [`DESC_READ`] bytes; a shorter device answer is
/// parsed as returned.
///
/// Error contract: `Errno::Io` — the transfer failed or moved nothing;
/// `Errno::NoEnt` — the descriptor parses but carries no axis
/// (NotStick); `Errno::Inval` — malformed or unsupported descriptor.
pub(crate) fn fetch_table(bus: &mut dyn ControlBus, iface: u8) -> KResult<HidTable> {
    let pkt = report_desc_request(iface, DESC_READ);
    let mut desc = [0u8; DESC_READ as usize];
    let n = bus.control(&pkt, Some(&mut desc), true)?;
    if n == 0 || n as usize > desc.len() {
        return Err(Errno::Io);
    }
    let table = parse_report_desc(&desc[..n as usize]).map_err(hid_err)?;
    // The parser already rejects axisless descriptors (NotStick); the
    // js-criterion is restated explicitly (ruling 3) so a session can
    // never arm a button-only device if the parser's guard moves.
    if !has_axes(&table) {
        return Err(Errno::NoEnt);
    }
    Ok(table)
}
