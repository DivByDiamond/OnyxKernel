use super::*;
use crate::drivers::virtio::VqUsedElem;

struct TestDevice {
    dev: InDev,
    _registers: Box<[u32; 128]>,
    avail: Box<VqAvail>,
    used: Box<VqUsed>,
    events: Box<[VirtioInputEvent; N_EVENTS]>,
}
impl TestDevice {
    fn new(code: u16) -> Self {
        let mut registers = Box::new([0u32; 128]);
        // SAFETY: virtqueue structs contain only integer fields/arrays.
        let mut avail: Box<VqAvail> = Box::new(unsafe { core::mem::zeroed() });
        let mut used: Box<VqUsed> = Box::new(unsafe { core::mem::zeroed() });
        let mut events = Box::new(
            [VirtioInputEvent {
                type_: 1,
                code,
                value: 1,
            }; N_EVENTS],
        );
        used.idx = 1;
        used.ring[0] = VqUsedElem { idx: 0, len: 8 };
        let dev = InDev {
            ready: true,
            base: registers.as_mut_ptr() as usize,
            avail: &mut *avail,
            used: &mut *used,
            ev_buf: events.as_mut_ptr(),
            ..EMPTY
        };
        Self {
            dev,
            _registers: registers,
            avail,
            used,
            events,
        }
    }
}

#[test]
fn multiple_queues_are_consumed_fairly_and_recycled_independently() {
    let a = TestDevice::new(30); // a
    let b = TestDevice::new(48); // b
    let mut inputs = InputDevices::new();
    inputs.devices[0] = a.dev;
    inputs.devices[7] = b.dev;
    assert!(matches!(
        unsafe { inputs.poll() },
        Some(decode::EventType::Key(
            crate::drivers::input::KeyCode::Letter(b'a'),
            true
        ))
    ));
    assert!(matches!(
        unsafe { inputs.poll() },
        Some(decode::EventType::Key(
            crate::drivers::input::KeyCode::Letter(b'b'),
            true
        ))
    ));
    assert!(unsafe { inputs.poll() }.is_none());
    assert_eq!(inputs.devices[0].last_used, 1);
    assert_eq!(inputs.devices[7].last_used, 1);
    assert_eq!(a.avail.idx, 1);
    assert_eq!(b.avail.idx, 1);
}

#[test]
fn a_busy_first_device_does_not_starve_a_second_keyboard() {
    let mut a = TestDevice::new(30);
    let b = TestDevice::new(48);
    a.used.idx = 2;
    a.used.ring[1] = VqUsedElem { idx: 1, len: 8 };
    a.events[1].code = 46;
    let mut inputs = InputDevices::new();
    inputs.devices[0] = a.dev;
    inputs.devices[1] = b.dev;
    assert!(matches!(
        unsafe { inputs.poll() },
        Some(decode::EventType::Key(
            crate::drivers::input::KeyCode::Letter(b'a'),
            true
        ))
    ));
    assert!(matches!(
        unsafe { inputs.poll() },
        Some(decode::EventType::Key(
            crate::drivers::input::KeyCode::Letter(b'b'),
            true
        ))
    ));
    assert!(matches!(
        unsafe { inputs.poll() },
        Some(decode::EventType::Key(
            crate::drivers::input::KeyCode::Letter(b'c'),
            true
        ))
    ));
}

#[test]
fn one_keyboard_release_does_not_release_another_keyboards_modifier_or_button() {
    let mut inputs = InputDevices::new();
    for code in [42, 29, 0x111] {
        let down = VirtioInputEvent {
            type_: 1,
            code,
            value: 1,
        };
        inputs.aggregate(0, down);
        inputs.aggregate(1, down);
        assert!(matches!(
            inputs.aggregate(0, VirtioInputEvent { value: 0, ..down }),
            decode::EventType::Syn
        ));
        assert!(!matches!(
            inputs.aggregate(1, VirtioInputEvent { value: 0, ..down }),
            decode::EventType::Syn
        ));
        assert!(matches!(
            inputs.aggregate(1, VirtioInputEvent { value: 0, ..down }),
            decode::EventType::Syn
        ));
        assert_eq!(inputs.counts[code as usize], 0);
    }
}

#[test]
fn malformed_completions_never_read_or_recycle_an_unowned_buffer() {
    let mut a = TestDevice::new(30);
    a.used.ring[0].idx = N_EVENTS as u32;
    assert!(matches!(
        unsafe { decode::poll_device(&mut a.dev) },
        Some(VirtioInputEvent { type_: 0, .. })
    ));
    assert_eq!(a.avail.idx, 0);
    a.dev.last_used = 0;
    a.used.ring[0] = VqUsedElem { idx: 0, len: 4 };
    assert!(matches!(
        unsafe { decode::poll_device(&mut a.dev) },
        Some(VirtioInputEvent { type_: 0, .. })
    ));
    assert_eq!(a.avail.idx, 1);
}

#[test]
fn used_ring_indices_wrap_without_losing_the_next_event() {
    let mut a = TestDevice::new(30);
    a.dev.last_used = u16::MAX;
    a.used.idx = 0;
    a.used.ring[u16::MAX as usize % VIRTQ_SIZE] = VqUsedElem { idx: 0, len: 8 };
    assert!(unsafe { decode::poll_device(&mut a.dev) }.is_some());
    assert_eq!(a.dev.last_used, 0);
    assert!(unsafe { decode::poll_device(&mut a.dev) }.is_none());
}

#[test]
fn device_registry_rejects_duplicates_but_accepts_distinct_endpoints_until_capacity() {
    let mut inputs = InputDevices::new();
    for i in 0..MAX_INPUT_DEVICES {
        let base = 0x10001000 + i * 0x1000;
        let slot = inputs.slot(base).unwrap();
        inputs.devices[slot].base = base;
        assert!(matches!(inputs.slot(base), Err(Errno::Busy)));
    }
    assert!(matches!(inputs.slot(0x20000000), Err(Errno::NoSpace)));
}

#[test]
fn a_full_staged_key_burst_has_buffers_for_every_event() {
    let mut device = TestDevice::new(30);
    let codes = [18, 46, 35, 24, 57, 48, 22, 19, 31, 20, 24, 37, 28];
    let mut index = 0;
    for code in codes {
        for (kind, value) in [(1, 1), (0, 0), (1, 0), (0, 0)] {
            assert!(index < N_EVENTS);
            device.events[index] = VirtioInputEvent {
                type_: kind,
                code: if kind == 1 { code } else { 0 },
                value,
            };
            device.used.ring[index] = VqUsedElem {
                idx: index as u32,
                len: 8,
            };
            index += 1;
        }
    }
    device.used.idx = index as u16;
    let mut inputs = InputDevices::new();
    inputs.devices[0] = device.dev;
    let mut presses = 0;
    while let Some(event) = unsafe { inputs.poll() } {
        if matches!(event, decode::EventType::Key(_, true)) {
            presses += 1;
        }
    }
    assert_eq!(presses, codes.len());
    assert_eq!(device.avail.idx as usize, index);
}

#[test]
fn absolute_pointer_decoder_ignores_unknown_axes_without_zeroing_coordinates() {
    use super::decode::{EV_ABS, EventType, translate};
    match translate(super::VirtioInputEvent {
        type_: EV_ABS,
        code: 0,
        value: 253,
    }) {
        EventType::MouseAbsX(253) => {}
        _ => panic!("ABS_X must update only X"),
    }
    match translate(super::VirtioInputEvent {
        type_: EV_ABS,
        code: 1,
        value: 159,
    }) {
        EventType::MouseAbsY(159) => {}
        _ => panic!("ABS_Y must update only Y"),
    }
    assert!(matches!(
        translate(super::VirtioInputEvent {
            type_: EV_ABS,
            code: 2,
            value: 50
        }),
        EventType::Syn
    ));
}
