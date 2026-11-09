use super::*;

#[test]
fn short_friendly_names_do_not_need_lfn() {
    assert!(!needs_lfn(b"README.TXT"));
    assert!(!needs_lfn(b"A.TXT"));
    assert!(!needs_lfn(b"NOEXT"));
    assert!(!needs_lfn(b"~SYS.TXT"));
}

#[test]
fn long_or_case_preserving_names_need_lfn() {
    assert!(needs_lfn(b"long name.txt")); // lowercase + space
    assert!(needs_lfn(b"README.TEXT")); // extension longer than 3
    assert!(needs_lfn(b"VERYLONGNAME.TXT")); // base longer than 8
    assert!(needs_lfn(b"a.b.c")); // more than one dot
    assert!(needs_lfn(b""));
}

#[test]
fn clean_alias_is_uppercased_without_a_tilde() {
    let mut out = [0u8; 11];
    build_short(b"README.TXT", false, 0, &mut out);
    assert_eq!(&out, b"README  TXT");
}

#[test]
fn lossy_alias_keeps_six_characters_and_a_tilde() {
    let mut out = [0u8; 11];
    build_short(b"long name.txt", true, 1, &mut out);
    assert_eq!(&out, b"LONGNA~1TXT");
}

#[test]
fn alias_numbers_past_nine_shrink_the_base_by_one() {
    let mut out = [0u8; 11];
    build_short(b"long name.txt", true, 12, &mut out);
    assert_eq!(&out, b"LONGN~12TXT");
}

#[test]
fn text_form_restores_the_extension_dot() {
    let mut text = [0u8; 12];
    let len = short_as_text(b"LONGNA~1TXT", &mut text);
    assert_eq!(&text[..len], b"LONGNA~1.TXT");
    let len = short_as_text(b"NOEXT      ", &mut text);
    assert_eq!(&text[..len], b"NOEXT");
}
