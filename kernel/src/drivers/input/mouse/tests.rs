use super::*;
#[test]
fn short_click_survives_motion_and_two_edges_between_reads() {
    let mut q = PointerQueue::new();
    q.handle(Event::MouseAbs { x: 30, y: 98 }, 256, 160);
    q.handle(
        Event::MouseButton {
            btn: super::super::event::MouseButton::Left,
            down: true,
        },
        256,
        160,
    );
    q.handle(Event::MouseAbsX(31), 256, 160);
    q.handle(
        Event::MouseButton {
            btn: super::super::event::MouseButton::Left,
            down: false,
        },
        256,
        160,
    );
    let press = q.read();
    let release = q.read();
    assert_eq!((press.x, press.y, press.buttons), (30, 98, 1));
    assert_eq!((release.x, release.y, release.buttons), (31, 98, 0));
    assert_eq!(q.read().buttons, 0);
}
#[test]
fn overflow_is_bounded_and_cancels_partial_gesture() {
    let mut q = PointerQueue::new();
    for i in 0..=EDGES {
        q.handle(
            Event::MouseButton {
                btn: super::super::event::MouseButton::Left,
                down: i % 2 == 0,
            },
            256,
            160,
        );
    }
    assert!(q.len <= EDGES);
    let cancel = q.read();
    assert_eq!((cancel.x, cancel.y, cancel.buttons), (-1, -1, 0));
}
#[test]
fn absolute_axes_preserve_each_other_and_clamp_to_tablet() {
    let mut m = MouseState {
        x: 0,
        y: 0,
        buttons: 0,
    };
    apply(&mut m, Event::MouseAbsX(120), 256, 160);
    apply(&mut m, Event::MouseAbsY(99), 256, 160);
    assert_eq!((m.x, m.y), (120, 99));
    apply(&mut m, Event::MouseAbsX(255), 256, 160);
    assert_eq!((m.x, m.y), (255, 99));
    apply(&mut m, Event::MouseAbsY(999), 256, 160);
    apply(&mut m, Event::MouseAbsX(-10), 256, 160);
    assert_eq!((m.x, m.y), (0, 159));
}
#[test]
fn all_eight_buttons_have_distinct_userspace_bits() {
    use crate::drivers::input::MouseButton;
    let mut bits = 0;
    for button in [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Other(3),
        MouseButton::Other(4),
        MouseButton::Other(5),
        MouseButton::Other(6),
        MouseButton::Other(7),
    ] {
        let bit = button_bit(button);
        assert_eq!(bits & bit, 0);
        assert_ne!(bit, 0);
        bits |= bit;
    }
    assert_eq!(bits, 255);
    assert_eq!(button_bit(MouseButton::Other(8)), 0);
}
