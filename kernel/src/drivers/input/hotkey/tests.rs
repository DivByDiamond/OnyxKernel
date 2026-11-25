use super::*;

/// Serializes the tests that touch the global table and the TTY FIFO.
static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn arg(id: u8, mods: u8, code: u16) -> u64 {
    u64::from(id) | (u64::from(mods) << 8) | (u64::from(code) << 16)
}

fn chord(id: u8, mods: u8, code: KeyCode) -> Chord {
    Chord { id, mods, code }
}

#[test]
fn chords_decode_only_when_every_field_is_valid() {
    assert_eq!(
        decode_chord(arg(1, MOD_ALT, 0x09)),
        Some(chord(1, MOD_ALT, KeyCode::Tab))
    );
    assert_eq!(
        decode_chord(arg(2, MOD_CTRL | MOD_SHIFT, CODE_F_BASE + 5)),
        Some(chord(2, MOD_CTRL | MOD_SHIFT, KeyCode::F(5)))
    );
    assert_eq!(
        decode_chord(arg(0, MOD_ALT, 0x09)),
        None,
        "id 0 is reserved"
    );
    assert_eq!(
        decode_chord(arg(1, 0x80, 0x09)),
        None,
        "unknown modifier bit"
    );
    assert_eq!(decode_chord(arg(1, 0, 0x7F)), None, "unknown key code");
    assert_eq!(
        decode_chord(arg(1, 0, CODE_F_BASE)),
        None,
        "F0 is not a key"
    );
    assert_eq!(decode_chord(arg(1, 0, CODE_F_BASE + 13)), None);
    assert_eq!(
        decode_chord(arg(1, 0, 0x09) | 1 << 40),
        None,
        "stray high bits"
    );
}

#[test]
fn modifier_flags_map_to_wire_bits_and_ignore_caps_lock() {
    assert_eq!(mods_from_flags(0), 0);
    assert_eq!(mods_from_flags(2), MOD_SHIFT);
    assert_eq!(mods_from_flags(8), MOD_CTRL);
    assert_eq!(mods_from_flags(32), MOD_ALT);
    assert_eq!(mods_from_flags(64), MOD_ALT);
    assert_eq!(mods_from_flags(16), 0, "caps lock is not a chord modifier");
    assert_eq!(mods_from_flags(1 | 4 | 32), MOD_SHIFT | MOD_CTRL | MOD_ALT);
}

#[test]
fn a_matching_keydown_is_swallowed_queued_once_and_exact_on_modifiers() {
    let mut t = Table::new();
    t.set(7, chord(1, MOD_ALT, KeyCode::Tab)).unwrap();
    assert_eq!(t.matches(KeyCode::Tab, MOD_ALT), Some(1));
    assert_eq!(
        t.matches(KeyCode::Tab, 0),
        None,
        "bare Tab is not the chord"
    );
    assert_eq!(t.matches(KeyCode::Tab, MOD_ALT | MOD_SHIFT), None);
    t.push(1);
    t.push(1); // auto-repeat collapses
    assert_eq!(t.pop(), Some(1));
    assert_eq!(t.pop(), None);
}

#[test]
fn the_queue_drops_newest_when_full_and_keeps_order() {
    let mut t = Table::new();
    for id in 1..=QUEUE_CAP as u8 {
        t.push(id);
    }
    t.push(99); // full: dropped, never overwrites
    for id in 1..=QUEUE_CAP as u8 {
        assert_eq!(t.pop(), Some(id));
    }
    assert_eq!(t.pop(), None);
}

#[test]
fn only_one_owner_and_the_table_is_bounded() {
    let mut t = Table::new();
    assert_eq!(t.set(0, chord(1, 0, KeyCode::Esc)), Err(Errno::Inval));
    t.set(5, chord(1, MOD_ALT, KeyCode::Tab)).unwrap();
    assert_eq!(t.set(6, chord(2, 0, KeyCode::Esc)), Err(Errno::Busy));
    // Same id replaces in place.
    t.set(5, chord(1, MOD_CTRL, KeyCode::Tab)).unwrap();
    assert_eq!(t.matches(KeyCode::Tab, MOD_ALT), None);
    assert_eq!(t.matches(KeyCode::Tab, MOD_CTRL), Some(1));
    for id in 2..=MAX_HOTKEYS as u8 {
        t.set(5, chord(id, 0, KeyCode::F(id))).unwrap();
    }
    assert_eq!(
        t.set(5, chord(9, 0, KeyCode::F(9))),
        Err(Errno::NoMem),
        "a fifth distinct id does not fit"
    );
}

/// Global-table path: intercept swallows (down and up), poll is
/// owner-only, clear by a stranger is a no-op, clear by the owner wipes.
#[test]
fn global_table_intercept_poll_and_clear() {
    let _g = GUARD.lock().unwrap();
    clear(4242);
    assert!(
        !intercept(KeyCode::Tab, true, 32),
        "no table, nothing swallowed"
    );
    register(4242, arg(3, MOD_ALT, 0x09)).unwrap();
    assert_eq!(register(1, arg(4, 0, 0x1B)), Err(Errno::Busy));
    assert_eq!(owner(), 4242);
    assert!(intercept(KeyCode::Tab, true, 32));
    assert!(
        intercept(KeyCode::Tab, false, 32),
        "key-up swallowed, not queued"
    );
    assert!(
        !intercept(KeyCode::Tab, true, 0),
        "bare Tab reaches the TTY"
    );
    assert_eq!(poll(1), Err(Errno::Perm));
    assert_eq!(poll(4242), Ok(3));
    assert_eq!(poll(4242), Ok(0));
    clear(1);
    assert_eq!(owner(), 4242);
    clear(4242);
    assert_eq!(owner(), 0);
    assert_eq!(poll(4242), Err(Errno::Perm));
}

/// End to end through `event::dispatch`: Alt+Tab never reaches the TTY and
/// queues the id, while plain Tab (and Alt alone) behave as before.
#[test]
fn dispatch_swallows_the_chord_before_the_tty_sees_it() {
    use crate::drivers::input::event::{Event, dispatch};
    use crate::drivers::input::tty;
    let _g = GUARD.lock().unwrap();
    clear(31337);
    while tty::pop().is_some() {}
    register(31337, arg(1, MOD_ALT, 0x09)).unwrap();
    let key = |code, down| dispatch(Event::Key { code, down });
    key(KeyCode::Tab, true); // bare Tab: ordinary byte
    key(KeyCode::Tab, false);
    assert_eq!(tty::pop(), Some(b'\t'));
    key(KeyCode::LeftAlt, true);
    key(KeyCode::Tab, true); // the chord
    key(KeyCode::Tab, false);
    key(KeyCode::LeftAlt, false);
    assert_eq!(tty::pop(), None, "no byte for a swallowed chord");
    assert_eq!(poll(31337), Ok(1));
    assert_eq!(poll(31337), Ok(0));
    key(KeyCode::Tab, true); // modifier released: ordinary again
    key(KeyCode::Tab, false);
    assert_eq!(tty::pop(), Some(b'\t'));
    clear(31337);
}
