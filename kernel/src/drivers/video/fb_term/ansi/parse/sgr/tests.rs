use super::{AnsiTerm, DEFAULT_BG, DEFAULT_FG};

fn feed(term: &mut AnsiTerm, bytes: &[u8]) {
    for &b in bytes {
        term.putc(b);
    }
}

#[test]
fn resets_to_normal_console_colors() {
    let mut term = AnsiTerm::new();
    feed(&mut term, b"\x1b[0m");
    assert_eq!(term.fg, DEFAULT_FG);
    assert_eq!(term.bg, DEFAULT_BG);
}

#[test]
fn maps_bright_and_basic_colors() {
    let mut term = AnsiTerm::new();
    feed(&mut term, b"\x1b[1;36m");
    assert_eq!(term.fg, 0x00_FF_FF);
    feed(&mut term, b"\x1b[1;101m");
    assert_eq!(term.bg, 0xFF_55_55);
}

#[test]
fn maps_extended_and_true_color() {
    let mut term = AnsiTerm::new();
    feed(&mut term, b"\x1b[38;5;196m");
    assert_eq!(term.fg, 0xFF_00_00);
    feed(&mut term, b"\x1b[48;2;16;32;64m");
    assert_eq!(term.bg, 0x10_20_40);
}
