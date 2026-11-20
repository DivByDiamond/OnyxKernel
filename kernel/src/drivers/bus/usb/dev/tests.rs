//! Scripted-bus tests for the enumeration sequence and the device slot
//! registry. The `FakeBus` double matches canned responses by PARSED
//! request fields (bmRequestType, bRequest, wValue), not raw 8-byte
//! equality, per the controller rulings.

use super::desc::tests::{config_fixture, device_fixture};
use super::desc::{CLASS_HID, DESC_CONFIGURATION, DESC_DEVICE};
use super::requests::{ControlBus, get_descriptor, set_address, set_configuration};
use super::{MAX_SLOTS, Owner, enumerate, set_slot, slot};
use onyx_core::errno::{Errno, KResult};

/// Device address assigned by the enumeration pass (ruling 3).
const ENUM_ADDR: u8 = 1;

/// One scripted control-transfer reply, matched by parsed fields.
struct Canned {
    rtype: u8,
    request: u8,
    value: u16,
    /// Bus address the request must arrive on (`None`: any address).
    at_addr: Option<u8>,
    reply: Reply,
}

enum Reply {
    /// Data for an IN transfer, copied up to the buffer's size.
    Data(std::vec::Vec<u8>),
    /// Zero-data OUT request succeeds.
    Ok,
    /// Fail the transfer with this errno.
    Err(Errno),
}

impl Canned {
    /// Script a reply for the request built by `pkt`, matching on the
    /// packet's parsed fields rather than its raw bytes. `at` is the
    /// bus address the request must arrive on (`None`: any).
    fn for_pkt(pkt: &[u8; 8], reply: Reply, at: Option<u8>) -> Self {
        let value = u16::from_le_bytes([pkt[2], pkt[3]]);
        Canned {
            rtype: pkt[0],
            request: pkt[1],
            value,
            at_addr: at,
            reply,
        }
    }
}

/// GET_DESCRIPTOR of `typ` served with `bytes`, at bus address `at`.
fn desc_step(bytes: &[u8], typ: u8, len: u16, at: Option<u8>) -> Canned {
    Canned::for_pkt(
        &get_descriptor(typ, 0, len),
        Reply::Data(bytes.to_vec()),
        at,
    )
}

/// A succeeding zero-data OUT request, at bus address `at`.
fn out_step(pkt: &[u8; 8], at: Option<u8>) -> Canned {
    Canned::for_pkt(pkt, Reply::Ok, at)
}

/// Scripted `ControlBus` double: each control call consumes the next
/// canned entry in order; a request that does not match its entry's
/// parsed fields (or the bus address), or an exhausted script, fails
/// with `Errno::Io`. `set_addr` re-targets the double, so entries
/// scripted with `at = Some(1)` enforce the ruling-3 re-address step.
struct FakeBus {
    addr: u8,
    script: std::vec::Vec<Canned>,
    cursor: usize,
}

impl FakeBus {
    fn new(script: std::vec::Vec<Canned>) -> Self {
        FakeBus {
            addr: 0,
            script,
            cursor: 0,
        }
    }
}

impl ControlBus for FakeBus {
    fn control(&mut self, setup: &[u8; 8], data: Option<&mut [u8]>, data_in: bool) -> KResult<u32> {
        let value = u16::from_le_bytes([setup[2], setup[3]]);
        let matched = match self.script.get(self.cursor) {
            Some(c) if c.rtype == setup[0] && c.request == setup[1] && c.value == value => {
                match c.at_addr {
                    Some(want) if want != self.addr => None,
                    _ => Some(c),
                }
            }
            _ => None,
        };
        let Some(c) = matched else {
            return Err(Errno::Io);
        };
        self.cursor += 1;
        match &c.reply {
            Reply::Err(e) => Err(*e),
            Reply::Ok => Ok(0),
            Reply::Data(bytes) => {
                if !data_in {
                    return Err(Errno::Io); // scripted IN in a zero-data request
                }
                let buf = data.ok_or(Errno::Io)?;
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                Ok(n as u32)
            }
        }
    }

    fn set_addr(&mut self, addr: u8) {
        self.addr = addr;
    }
}

/// Full happy-path script over `cfg`: the descriptor reads happen at
/// the default address 0, SET_ADDRESS(1) re-targets the bus, and every
/// later step must arrive on the re-addressed bus (ruling 3).
fn script_with_config(cfg: &[u8; 25]) -> std::vec::Vec<Canned> {
    let dev = device_fixture();
    let after = Some(ENUM_ADDR);
    std::vec::Vec::from([
        desc_step(&dev, DESC_DEVICE, 8, None),
        out_step(&set_address(ENUM_ADDR), None),
        desc_step(&dev, DESC_DEVICE, 18, after),
        desc_step(cfg, DESC_CONFIGURATION, 9, after),
        desc_step(cfg, DESC_CONFIGURATION, 25, after),
        out_step(&set_configuration(1), after),
    ])
}

#[test]
fn enumerate_happy_path_yields_hid_slot() {
    let cfg = config_fixture();
    let mut bus = FakeBus::new(script_with_config(&cfg));
    let s = enumerate(&mut bus, Owner::Ohci).unwrap();
    assert!(s.active);
    assert_eq!(s.owner, Owner::Ohci); // owner passed through as-is
    assert_eq!(s.addr, ENUM_ADDR);
    assert_eq!(s.vendor, 0x044f);
    assert_eq!(s.product, 0xb10a);
    assert_eq!(s.class, CLASS_HID);
    assert_eq!(s.iface, 0);
    assert_eq!(s.ep_in, 0x81);
    assert_eq!(s.max_packet, 8);
    assert_eq!(s.interval, 10);
    // Whole script consumed; the bus ended re-targeted at the address.
    assert_eq!(bus.cursor, bus.script.len());
    assert_eq!(bus.addr, ENUM_ADDR);
}

/// The one test that touches the shared registry: everything it asserts
/// runs sequentially inside it, so parallel test harts cannot interfere.
#[test]
fn enumerate_bus_error_keeps_registry_clean_for_the_next_pass() {
    // The bus fails at the third request (the full device-descriptor
    // read) — enumeration reports the error and takes no slot.
    let mut bad = script_with_config(&config_fixture());
    bad[2].reply = Reply::Err(Errno::Io);
    let mut bus = FakeBus::new(bad);
    assert_eq!(enumerate(&mut bus, Owner::Ehci), Err(Errno::Io));
    assert_eq!(slot(0), None);

    // A second, successful pass registers into the first slot: the
    // failure left no garbage behind.
    let cfg = config_fixture();
    let mut good = FakeBus::new(script_with_config(&cfg));
    let s = enumerate(&mut good, Owner::Ehci).unwrap();
    assert_eq!(set_slot(0, s), Ok(()));
    let stored = slot(0).unwrap();
    assert_eq!(
        (stored.owner, stored.addr, stored.class, stored.ep_in),
        (Owner::Ehci, ENUM_ADDR, CLASS_HID, 0x81)
    );

    // Registry discipline: active-over-active and out-of-range are
    // refused; a deactivation frees the slot again.
    assert_eq!(set_slot(0, s), Err(Errno::Busy));
    assert_eq!(set_slot(MAX_SLOTS, s), Err(Errno::Range));
    assert_eq!(slot(MAX_SLOTS), None);
    let mut inactive = s;
    inactive.active = false;
    assert_eq!(set_slot(0, inactive), Ok(()));
    assert_eq!(slot(0), None);
}

/// Hostile bus responses: truncated reads and malformed head bytes are
/// reported as errors, never panics.
#[test]
fn enumerate_hostile_bus_responses_are_err() {
    // Truncated 8-byte head read.
    let mut script = script_with_config(&config_fixture());
    script[0].reply = Reply::Data(device_fixture()[..6].to_vec());
    let mut bus = FakeBus::new(script);
    assert_eq!(enumerate(&mut bus, Owner::Xhci), Err(Errno::Io));
    // Truncated 18-byte device-descriptor read.
    let mut script = script_with_config(&config_fixture());
    script[2].reply = Reply::Data(device_fixture()[..10].to_vec());
    let mut bus = FakeBus::new(script);
    assert_eq!(enumerate(&mut bus, Owner::Xhci), Err(Errno::Io));
    // Truncated 9-byte configuration header read.
    let mut script = script_with_config(&config_fixture());
    script[3].reply = Reply::Data(config_fixture()[..5].to_vec());
    let mut bus = FakeBus::new(script);
    assert_eq!(enumerate(&mut bus, Owner::Xhci), Err(Errno::Io));
    // Wrong bDescriptorType in the head read.
    let mut dev = device_fixture();
    dev[1] = DESC_CONFIGURATION;
    let mut script = script_with_config(&config_fixture());
    script[0].reply = Reply::Data(dev.to_vec());
    let mut bus = FakeBus::new(script);
    assert_eq!(enumerate(&mut bus, Owner::Xhci), Err(Errno::Inval));
    // bMaxPacketSize0 outside the legal EP0 sizes {8, 16, 32, 64}.
    let mut dev = device_fixture();
    dev[7] = 7;
    let mut script = script_with_config(&config_fixture());
    script[0].reply = Reply::Data(dev.to_vec());
    let mut bus = FakeBus::new(script);
    assert_eq!(enumerate(&mut bus, Owner::Xhci), Err(Errno::Inval));
}

/// A configuration without a HID interrupt-IN interface is `NoEnt`
/// (ruling 4).
#[test]
fn enumerate_config_without_hid_interrupt_in_is_noent() {
    // A mass-storage interface instead of HID: nothing to pick.
    let mut cfg = config_fixture();
    cfg[14] = 0x08;
    let mut bus = FakeBus::new(script_with_config(&cfg));
    assert_eq!(enumerate(&mut bus, Owner::Xhci), Err(Errno::NoEnt));
    // HID interface whose only endpoint is bulk IN — not interrupt-IN.
    let mut cfg = config_fixture();
    cfg[21] = 0x02;
    let mut bus = FakeBus::new(script_with_config(&cfg));
    assert_eq!(enumerate(&mut bus, Owner::Xhci), Err(Errno::NoEnt));
    // HID interface whose only endpoint is interrupt OUT — right type,
    // wrong direction.
    let mut cfg = config_fixture();
    cfg[20] = 0x01;
    let mut bus = FakeBus::new(script_with_config(&cfg));
    assert_eq!(enumerate(&mut bus, Owner::Xhci), Err(Errno::NoEnt));
}
