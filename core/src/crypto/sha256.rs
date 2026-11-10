//! SHA-256 (FIPS 180-4). Streaming API: `sha256_init` / `sha256_update` /
//! `sha256_final`, plus the one-shot `sha256` convenience wrapper. The
//! compression core (fully unrolled rounds) lives in `sha256/compress.rs`.
//!
//! Correctness is pinned by known-answer tests (RFC 6234 vectors and block
//! boundary cases) living in `tests/sha256.rs`; they run on the host via
//! `cargo test -p onyx_core`.

mod compress;

use compress::sha256_compress;

pub struct Sha256State {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

pub fn sha256_init() -> Sha256State {
    Sha256State {
        state: [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ],
        buf: [0u8; 64],
        buf_len: 0,
        total_len: 0,
    }
}

pub fn sha256_update(state: &mut Sha256State, data: &[u8]) {
    state.total_len = state.total_len.wrapping_add(data.len() as u64);
    let mut data = data;

    // Top up a partially filled buffer first; a single compression runs only
    // when the buffer becomes exactly full.
    if state.buf_len > 0 {
        let room = 64 - state.buf_len;
        let take = room.min(data.len());
        state.buf[state.buf_len..state.buf_len + take].copy_from_slice(&data[..take]);
        state.buf_len += take;
        data = &data[take..];
        if state.buf_len < 64 {
            return;
        }
        sha256_compress(&mut state.state, &state.buf);
        state.buf_len = 0;
    }

    // Whole blocks are compressed straight out of the caller's slice: no
    // per-byte staging copy (the old loop cost ~13 instructions per byte).
    let (blocks, rest) = data.as_chunks::<64>();
    for block in blocks {
        sha256_compress(&mut state.state, block);
    }
    state.buf[..rest.len()].copy_from_slice(rest);
    state.buf_len = rest.len();
}

pub fn sha256_final(state: Sha256State) -> [u8; 32] {
    let mut state = state;
    let bit_len = state.total_len.wrapping_mul(8);

    state.buf[state.buf_len] = 0x80;
    state.buf_len += 1;

    while state.buf_len % 64 != 56 {
        if state.buf_len == 64 {
            sha256_compress(&mut state.state, &state.buf);
            state.buf_len = 0;
        }
        state.buf[state.buf_len] = 0;
        state.buf_len += 1;
    }

    state.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
    sha256_compress(&mut state.state, &state.buf);

    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..(i + 1) * 4].copy_from_slice(&state.state[i].to_be_bytes());
    }
    out
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = sha256_init();
    sha256_update(&mut state, data);
    sha256_final(state)
}
