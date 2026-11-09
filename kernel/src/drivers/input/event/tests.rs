use super::*;
#[test]
fn a_reentrant_drain_cannot_dispatch_newer_events_before_the_current_one() {
    let lock = crate::sync::SpinLock::new();
    let mut events = [
        virtio_input::EventType::Key(KeyCode::LeftShift, true),
        virtio_input::EventType::Key(KeyCode::Letter(b'a'), true),
    ]
    .into_iter();
    let mut received = std::vec::Vec::new();
    drain_with(
        &lock,
        || events.next(),
        |ev| {
            drain_with(
                &lock,
                || panic!("reentrant poll must not consume events"),
                |_| panic!(),
            );
            if let Event::Key { code, .. } = ev {
                received.push(code);
            }
        },
    );
    assert_eq!(received, [KeyCode::LeftShift, KeyCode::Letter(b'a')]);
    assert!(lock.try_lock());
    lock.unlock();
}

#[test]
fn either_shift_and_ctrl_remain_active_until_both_sides_are_released() {
    let flags = AtomicU8::new(0);
    update_modifiers(&flags, KeyCode::LeftShift, true);
    update_modifiers(&flags, KeyCode::RightShift, true);
    update_modifiers(&flags, KeyCode::LeftShift, false);
    assert_ne!(flags.load(Ordering::Acquire) & 3, 0);
    update_modifiers(&flags, KeyCode::RightShift, false);
    assert_eq!(flags.load(Ordering::Acquire) & 3, 0);
    update_modifiers(&flags, KeyCode::LeftCtrl, true);
    update_modifiers(&flags, KeyCode::RightCtrl, true);
    update_modifiers(&flags, KeyCode::RightCtrl, false);
    assert_ne!(flags.load(Ordering::Acquire) & 12, 0);
    update_modifiers(&flags, KeyCode::LeftCtrl, false);
    assert_eq!(flags.load(Ordering::Acquire), 0);
}
