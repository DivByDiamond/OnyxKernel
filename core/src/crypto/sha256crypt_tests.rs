//! SHA-256 crypt tests. Every vector below was produced by an independent
//! implementation (`perl -e 'crypt(...)'` against the system libxcrypt, which
//! is the same code path as glibc's `crypt(3)`), so matching them proves
//! `$5$` interoperability rather than self-consistency.

use super::*;

/// Format `password`/`salt`/`rounds` and compare with the reference string.
fn assert_hash(password: &[u8], salt: &[u8], rounds: u32, expected: &str) {
    let mut buf = [0u8; 128];
    let len = hash_into(password, salt, rounds, &mut buf).expect("hash fits");
    assert_eq!(&buf[..len], expected.as_bytes());
}

#[test]
fn default_rounds_match_the_reference_vector() {
    // perl -e 'crypt("password", q{$5$saltsalt$})'
    let expected = "$5$saltsalt$gOjOtoMpVhru2uyjeJSEc/JaLQWOXMNmlOnj6T4AtC.";
    assert_hash(b"password", b"saltsalt", DEFAULT_ROUNDS, expected);
    assert!(verify(b"password", expected.as_bytes()));
    assert!(!verify(b"Password", expected.as_bytes()));
}

#[test]
fn explicit_rounds_are_honoured() {
    // perl -e 'crypt("password", q{$5$rounds=1000$saltsalt$})'
    let expected = "$5$rounds=1000$saltsalt$azOwbpkvuuBKkE82dQPwTsQE8JyT9Fflpr9aKid3aT9";
    assert_hash(b"password", b"saltsalt", 1_000, expected);
}

#[test]
fn explicit_default_rounds_tag_is_accepted_on_verify() {
    // perl -e 'crypt("Hello world!", q{$5$rounds=5000$saltstring$})'
    let stored = "$5$rounds=5000$saltstring$5B8vYYiY.CVt1RlTTf8KbXBH3hsxY/GNooZaBBGWEc5";
    assert!(verify(b"Hello world!", stored.as_bytes()));
    assert!(!verify(b"Hello world", stored.as_bytes()));
}

#[test]
fn empty_password_hashes_like_the_reference() {
    // perl -e 'crypt("", q{$5$rounds=1234$empty$})'
    let expected = "$5$rounds=1234$empty$1BshC1lup8UzkqRYsK/NrsUIgMNtqEaw5aAiJ4eypSD";
    assert_hash(b"", b"empty", 1_234, expected);
    assert!(verify(b"", expected.as_bytes()));
}

#[test]
fn long_password_recycling_matches_the_reference() {
    // 45 bytes: exercises the recycled P/S updates in every branch.
    // perl -e 'crypt($pw, q{$5$0123456789abcdef$})'
    let password = b"pneumonoultramicroscopicsilicovolcanoconiosis";
    let expected = "$5$0123456789abcdef$A5AbL2s1voeza6sY/hgpLaoNokRN0EWDFocT8T.xcE.";
    assert_hash(password, b"0123456789abcdef", DEFAULT_ROUNDS, expected);
    assert!(verify(password, expected.as_bytes()));
}

#[test]
fn utf8_password_matches_the_reference() {
    // A mixed non-ASCII password ("Gruesse" with umlauts plus Cyrillic),
    // written as escape bytes so this source file stays ASCII.
    let password: &[u8] =
        b"Gr\xc3\xbc\xc3\x9fe-\xd0\xbf\xd0\xb0\xd1\x80\xd0\xbe\xd0\xbb\xd1\x8c-test";
    let expected = "$5$rounds=10000$abcdefghijkl$pY49YEAQOSTtYqSLViFH0RiH9WEVDhJOiUNecgn/cD9";
    assert_hash(password, b"abcdefghijkl", 10_000, expected);
    assert!(verify(password, expected.as_bytes()));
}

#[test]
fn generated_hash_round_trips() {
    let mut buf = [0u8; 128];
    let len = hash_into(b"s3cret", b"0123456789abcdef", DEFAULT_ROUNDS, &mut buf).unwrap();
    let stored = &buf[..len];
    assert!(verify(b"s3cret", stored));
    assert!(!verify(b"wrong", stored));
    // Default rounds are omitted from the generated string.
    assert!(stored.starts_with(b"$5$0123456789abcdef$"));
}

#[test]
fn malformed_settings_are_rejected() {
    // Legacy Onyx hex layout (64-char tail) is not a $5$ setting.
    let legacy =
        b"$5$0011223344556677$0000000000000000000000000000000000000000000000000000000000000000";
    assert!(parse(legacy).is_none());
    assert!(parse(b"$6$saltsalt$gOjOtoMpVhru2uyjeJSEc/JaLQWOXMNmlOnj6T4AtC.").is_none());
    assert!(parse(b"$5$saltsalt$short").is_none());
    // Rounds out of range or with a leading zero are rejected, like libxcrypt.
    assert!(parse(b"$5$rounds=500$saltsalt$gOjOtoMpVhru2uyjeJSEc/JaLQWOXMNmlOnj6T4AtC.").is_none());
    assert!(
        parse(b"$5$rounds=05000$saltsalt$gOjOtoMpVhru2uyjeJSEc/JaLQWOXMNmlOnj6T4AtC.").is_none()
    );
    assert!(parse(b"$5$rounds=$saltsalt$gOjOtoMpVhru2uyjeJSEc/JaLQWOXMNmlOnj6T4AtC.").is_none());
    // Salt with a passwd separator is rejected.
    assert!(parse(b"$5$bad:salt$gOjOtoMpVhru2uyjeJSEc/JaLQWOXMNmlOnj6T4AtC.").is_none());
}

#[test]
fn salt_longer_than_sixteen_is_truncated() {
    let stored = "$5$abcdefghijklmnopEXTRA$A5AbL2s1voeza6sY/hgpLaoNokRN0EWDFocT8T.xcE.";
    let parsed = parse(stored.as_bytes()).expect("parses with truncated salt");
    assert_eq!(parsed.salt, b"abcdefghijklmnop");
}

#[test]
fn encoding_produces_only_alphabet_characters() {
    let digest = digest(b"password", b"saltsalt", DEFAULT_ROUNDS);
    let mut out = [0u8; HASH_CHARS];
    encode(&digest, &mut out).unwrap();
    assert!(out.iter().all(|b| B64.contains(b)));
    // 11 groups of 4 + 1 group of 3 = 43 characters.
    assert_eq!(out.len(), HASH_CHARS);
}
