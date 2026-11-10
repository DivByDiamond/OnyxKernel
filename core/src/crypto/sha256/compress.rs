//! SHA-256 compression core: 16-word rolling message schedule, 64 rounds
//! fully unrolled so every `K[t]`/`w[t]` index is a compile-time constant.
//!
//! Kept separate from the streaming API in `sha256.rs` because the unrolled
//! round sequence is long; `sha256_compress` is the only hot function here.

/// FIPS 180-4 round constants.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

#[inline(always)]
fn ch(x: u32, y: u32, z: u32) -> u32 {
    z ^ (x & (y ^ z))
}

#[inline(always)]
fn maj(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (z & (x | y))
}

#[inline(always)]
fn big_sigma0(x: u32) -> u32 {
    x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
}

#[inline(always)]
fn big_sigma1(x: u32) -> u32 {
    x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
}

#[inline(always)]
fn small_sigma0(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}

#[inline(always)]
fn small_sigma1(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}

/// One round, honouring FIPS rotation of `a..h`: the caller passes the
/// variables in rotated order, so no register shuffling is emitted.
macro_rules! round {
    ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $f:ident, $g:ident, $h:ident, $k:expr, $w:expr) => {{
        let t1 = $h
            .wrapping_add(big_sigma1($e))
            .wrapping_add(ch($e, $f, $g))
            .wrapping_add($k)
            .wrapping_add($w);
        let t2 = big_sigma0($a).wrapping_add(maj($a, $b, $c));
        $d = $d.wrapping_add(t1);
        $h = t1.wrapping_add(t2);
    }};
}

/// Extend the rolling 16-word schedule: `w[t & 15]` becomes `w[t]`.
macro_rules! extend {
    ($w:ident, $t:expr) => {{
        let i = $t & 15;
        $w[i] = $w[i]
            .wrapping_add(small_sigma0($w[($t - 15) & 15]))
            .wrapping_add($w[($t - 7) & 15])
            .wrapping_add(small_sigma1($w[($t - 2) & 15]));
    }};
}

/// Compress one 64-byte block into `state`.
pub(super) fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    // Explicit constant indices: no zero-fill of `w` and no load loop.
    let mut w: [u32; 16] = [
        u32::from_be_bytes([block[0], block[1], block[2], block[3]]),
        u32::from_be_bytes([block[4], block[5], block[6], block[7]]),
        u32::from_be_bytes([block[8], block[9], block[10], block[11]]),
        u32::from_be_bytes([block[12], block[13], block[14], block[15]]),
        u32::from_be_bytes([block[16], block[17], block[18], block[19]]),
        u32::from_be_bytes([block[20], block[21], block[22], block[23]]),
        u32::from_be_bytes([block[24], block[25], block[26], block[27]]),
        u32::from_be_bytes([block[28], block[29], block[30], block[31]]),
        u32::from_be_bytes([block[32], block[33], block[34], block[35]]),
        u32::from_be_bytes([block[36], block[37], block[38], block[39]]),
        u32::from_be_bytes([block[40], block[41], block[42], block[43]]),
        u32::from_be_bytes([block[44], block[45], block[46], block[47]]),
        u32::from_be_bytes([block[48], block[49], block[50], block[51]]),
        u32::from_be_bytes([block[52], block[53], block[54], block[55]]),
        u32::from_be_bytes([block[56], block[57], block[58], block[59]]),
        u32::from_be_bytes([block[60], block[61], block[62], block[63]]),
    ];

    let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
    let (mut e, mut f, mut g, mut h) = (state[4], state[5], state[6], state[7]);

    round!(a, b, c, d, e, f, g, h, K[0], w[0]);
    round!(h, a, b, c, d, e, f, g, K[1], w[1]);
    round!(g, h, a, b, c, d, e, f, K[2], w[2]);
    round!(f, g, h, a, b, c, d, e, K[3], w[3]);
    round!(e, f, g, h, a, b, c, d, K[4], w[4]);
    round!(d, e, f, g, h, a, b, c, K[5], w[5]);
    round!(c, d, e, f, g, h, a, b, K[6], w[6]);
    round!(b, c, d, e, f, g, h, a, K[7], w[7]);
    round!(a, b, c, d, e, f, g, h, K[8], w[8]);
    round!(h, a, b, c, d, e, f, g, K[9], w[9]);
    round!(g, h, a, b, c, d, e, f, K[10], w[10]);
    round!(f, g, h, a, b, c, d, e, K[11], w[11]);
    round!(e, f, g, h, a, b, c, d, K[12], w[12]);
    round!(d, e, f, g, h, a, b, c, K[13], w[13]);
    round!(c, d, e, f, g, h, a, b, K[14], w[14]);
    round!(b, c, d, e, f, g, h, a, K[15], w[15]);
    extend!(w, 16);
    round!(a, b, c, d, e, f, g, h, K[16], w[0]);
    extend!(w, 17);
    round!(h, a, b, c, d, e, f, g, K[17], w[1]);
    extend!(w, 18);
    round!(g, h, a, b, c, d, e, f, K[18], w[2]);
    extend!(w, 19);
    round!(f, g, h, a, b, c, d, e, K[19], w[3]);
    extend!(w, 20);
    round!(e, f, g, h, a, b, c, d, K[20], w[4]);
    extend!(w, 21);
    round!(d, e, f, g, h, a, b, c, K[21], w[5]);
    extend!(w, 22);
    round!(c, d, e, f, g, h, a, b, K[22], w[6]);
    extend!(w, 23);
    round!(b, c, d, e, f, g, h, a, K[23], w[7]);
    extend!(w, 24);
    round!(a, b, c, d, e, f, g, h, K[24], w[8]);
    extend!(w, 25);
    round!(h, a, b, c, d, e, f, g, K[25], w[9]);
    extend!(w, 26);
    round!(g, h, a, b, c, d, e, f, K[26], w[10]);
    extend!(w, 27);
    round!(f, g, h, a, b, c, d, e, K[27], w[11]);
    extend!(w, 28);
    round!(e, f, g, h, a, b, c, d, K[28], w[12]);
    extend!(w, 29);
    round!(d, e, f, g, h, a, b, c, K[29], w[13]);
    extend!(w, 30);
    round!(c, d, e, f, g, h, a, b, K[30], w[14]);
    extend!(w, 31);
    round!(b, c, d, e, f, g, h, a, K[31], w[15]);
    extend!(w, 32);
    round!(a, b, c, d, e, f, g, h, K[32], w[0]);
    extend!(w, 33);
    round!(h, a, b, c, d, e, f, g, K[33], w[1]);
    extend!(w, 34);
    round!(g, h, a, b, c, d, e, f, K[34], w[2]);
    extend!(w, 35);
    round!(f, g, h, a, b, c, d, e, K[35], w[3]);
    extend!(w, 36);
    round!(e, f, g, h, a, b, c, d, K[36], w[4]);
    extend!(w, 37);
    round!(d, e, f, g, h, a, b, c, K[37], w[5]);
    extend!(w, 38);
    round!(c, d, e, f, g, h, a, b, K[38], w[6]);
    extend!(w, 39);
    round!(b, c, d, e, f, g, h, a, K[39], w[7]);
    extend!(w, 40);
    round!(a, b, c, d, e, f, g, h, K[40], w[8]);
    extend!(w, 41);
    round!(h, a, b, c, d, e, f, g, K[41], w[9]);
    extend!(w, 42);
    round!(g, h, a, b, c, d, e, f, K[42], w[10]);
    extend!(w, 43);
    round!(f, g, h, a, b, c, d, e, K[43], w[11]);
    extend!(w, 44);
    round!(e, f, g, h, a, b, c, d, K[44], w[12]);
    extend!(w, 45);
    round!(d, e, f, g, h, a, b, c, K[45], w[13]);
    extend!(w, 46);
    round!(c, d, e, f, g, h, a, b, K[46], w[14]);
    extend!(w, 47);
    round!(b, c, d, e, f, g, h, a, K[47], w[15]);
    extend!(w, 48);
    round!(a, b, c, d, e, f, g, h, K[48], w[0]);
    extend!(w, 49);
    round!(h, a, b, c, d, e, f, g, K[49], w[1]);
    extend!(w, 50);
    round!(g, h, a, b, c, d, e, f, K[50], w[2]);
    extend!(w, 51);
    round!(f, g, h, a, b, c, d, e, K[51], w[3]);
    extend!(w, 52);
    round!(e, f, g, h, a, b, c, d, K[52], w[4]);
    extend!(w, 53);
    round!(d, e, f, g, h, a, b, c, K[53], w[5]);
    extend!(w, 54);
    round!(c, d, e, f, g, h, a, b, K[54], w[6]);
    extend!(w, 55);
    round!(b, c, d, e, f, g, h, a, K[55], w[7]);
    extend!(w, 56);
    round!(a, b, c, d, e, f, g, h, K[56], w[8]);
    extend!(w, 57);
    round!(h, a, b, c, d, e, f, g, K[57], w[9]);
    extend!(w, 58);
    round!(g, h, a, b, c, d, e, f, K[58], w[10]);
    extend!(w, 59);
    round!(f, g, h, a, b, c, d, e, K[59], w[11]);
    extend!(w, 60);
    round!(e, f, g, h, a, b, c, d, K[60], w[12]);
    extend!(w, 61);
    round!(d, e, f, g, h, a, b, c, K[61], w[13]);
    extend!(w, 62);
    round!(c, d, e, f, g, h, a, b, K[62], w[14]);
    extend!(w, 63);
    round!(b, c, d, e, f, g, h, a, K[63], w[15]);

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}
