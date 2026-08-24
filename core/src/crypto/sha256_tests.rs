//! Known-answer tests for `crypto::sha256` (RFC 6234 + block boundary
//! cases). Kept in a sibling file so the implementation stays within the
//! project's 250-line file limit.

use super::*;

/// Decode 64 lowercase hex chars into a digest (no_std-friendly: avoids
/// `format!`/`String` in tests).
fn unhex(expected: &str) -> [u8; 32] {
    let b = expected.as_bytes();
    assert_eq!(b.len(), 64);
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = (b[i * 2] as char).to_digit(16).unwrap() as u8;
        let lo = (b[i * 2 + 1] as char).to_digit(16).unwrap() as u8;
        out[i] = (hi << 4) | lo;
    }
    out
}

fn kat(msg: &[u8], expected: &str) {
    assert_eq!(sha256(msg), unhex(expected), "one-shot KAT failed");
    // Same digest must also come out of the streaming API.
    let mut st = sha256_init();
    sha256_update(&mut st, msg);
    assert_eq!(sha256_final(st), unhex(expected), "streaming KAT failed");
}

#[test]
fn rfc6234_empty() {
    kat(
        b"",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
}

#[test]
fn rfc6234_abc() {
    kat(
        b"abc",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
}

/// RFC 6234 two-block vector ("abcdbcde..." — exactly 56 bytes).
#[test]
fn rfc6234_two_blocks() {
    kat(
        b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    );
}

/// Block boundary lengths: padding lands in the same block (55), exactly
/// fills it (56/64), or spills into an extra block (57/63).
#[test]
fn boundary_lengths() {
    kat(
        &[b'a'; 55],
        "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
    );
    kat(
        &[b'a'; 56],
        "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
    );
    kat(
        &[b'a'; 57],
        "f13b2d724659eb3bf47f2dd6af1accc87b81f09f59f2b75e5c0bed6589dfe8c6",
    );
    kat(
        &[b'a'; 63],
        "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
    );
    kat(
        &[b'a'; 64],
        "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
    );
}

/// Feed one byte at a time to stress the internal buffer flush path against
/// a single-shot reference over the same data.
#[test]
fn chunked_update_matches_one_shot() {
    let data: [u8; 100] = core::array::from_fn(|i| (i * 37 % 251) as u8);
    let mut st = sha256_init();
    for b in &data {
        sha256_update(&mut st, core::slice::from_ref(b));
    }
    let streamed = sha256_final(st);
    assert_eq!(streamed, sha256(&data));
}
