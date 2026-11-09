//! PSF font loading and text rendering for userland widgets (todo P3 #4).
//!
//! Loads /font/default.psf (the same file the kernel console uses) and
//! blits glyphs into a raw 32bpp framebuffer slice. Both PSF1 (8xN) and
//! PSF2 (arbitrary width, MSB-first rows) are supported, mirroring the
//! kernel font parsers in kernel/src/font/.

use crate::syscalls;

const FONT_PATH: &[u8] = b"/font/default.psf\0";
const FONT_BUF_LEN: usize = 16384;

const PSF1_MAGIC: u16 = 0x0436;
const PSF2_MAGIC: u32 = 0x864ab572;

static mut FONT_BUF: [u8; FONT_BUF_LEN] = [0; FONT_BUF_LEN];
static mut FONT_LEN: usize = 0;
static mut G_FONT: Option<Font> = None;

/// Parsed font metadata pointing into FONT_BUF.
#[derive(Clone, Copy)]
pub struct Font {
    glyphs_start: usize,
    num_glyphs: usize,
    pub charsize: usize,
    pub width: usize,
    pub height: usize,
    psf1: bool,
}

/// Read the whole font file into the static buffer and parse the header.
/// Call once at startup; onyxA bins are single-threaded.
pub fn init() {
    // SAFETY: single-threaded startup path owns the static buffer; all
    // later reads go through raw pointers derived from it.
    unsafe {
        let fd = syscalls::open(FONT_PATH.as_ptr(), 0, 0);
        if fd < 0 {
            (&raw mut G_FONT).write(None);
            return;
        }
        let n = syscalls::read(
            fd as u64,
            (&raw mut FONT_BUF) as *mut u8,
            FONT_BUF_LEN as u64,
        );
        syscalls::close(fd as u64);
        if n <= 0 {
            (&raw mut G_FONT).write(None);
            return;
        }
        FONT_LEN = n as usize;
        (&raw mut G_FONT).write(parse(n as usize));
    }
}

/// Parse PSF1/PSF2 headers (bounds-checked; malformed fonts yield None).
fn parse(len: usize) -> Option<Font> {
    // SAFETY: callers pass offsets < 32 into FONT_BUF, which holds at least
    // `len` >= 32 validated bytes when this fn is called from init().
    let rd = |off: usize| -> u32 {
        unsafe {
            let p = (&raw const FONT_BUF as *const u8).add(off);
            (*p as u32)
                | ((*p.add(1) as u32) << 8)
                | ((*p.add(2) as u32) << 16)
                | ((*p.add(3) as u32) << 24)
        }
    };
    let is_psf1 = len >= 4 && rd(0) & 0xFFFF == PSF1_MAGIC as u32;
    let is_psf2 = len >= 32 && rd(0) == PSF2_MAGIC;
    if !is_psf1 && !is_psf2 {
        return None;
    }
    if is_psf1 {
        // PSF1: 4-byte header {magic u16, mode u8, charsize u8}, 8px wide,
        // charsize bytes per glyph, glyphs start at offset 4.
        let charsize = (rd(0) >> 24) as usize;
        if charsize == 0 || 4 + charsize > len {
            return None;
        }
        let num_glyphs = (len - 4) / charsize;
        return Some(Font {
            glyphs_start: 4,
            num_glyphs,
            charsize,
            width: 8,
            height: charsize,
            psf1: true,
        });
    }
    // PSF2 header layout (little-endian u32 fields).
    let hdr = rd(8) as usize;
    let num_glyphs = rd(16) as usize;
    let charsize = rd(20) as usize;
    let height = rd(24) as usize;
    let width = rd(28) as usize;
    if hdr == 0
        || hdr > len
        || num_glyphs == 0
        || charsize == 0
        || height == 0
        || width == 0
        || width % 8 != 0
        || num_glyphs.saturating_mul(charsize) > len.saturating_sub(hdr)
    {
        return None;
    }
    Some(Font {
        glyphs_start: hdr,
        num_glyphs,
        charsize,
        width,
        height,
        psf1: false,
    })
}

/// Loaded font (if any). Call init() first.
pub fn current() -> Option<&'static Font> {
    // SAFETY: read-only access to the static initialized by init(); no
    // mutable access happens after startup in a single-threaded bin.
    unsafe { (&raw const G_FONT).as_ref().and_then(|f| f.as_ref()) }
}

/// Length of valid data in FONT_BUF (set by init()).
fn font_len() -> usize {
    // SAFETY: read-only snapshot of a static written once by init().
    unsafe { *(&raw const FONT_LEN) }
}

/// Copy of the glyph bytes for `c`, or None when out of range. Reads go
/// through the raw static pointer (2024-edition static-mut rules).
fn glyph_row(font: &Font, c: u8, row: usize) -> Option<u8> {
    if c as usize >= font.num_glyphs {
        return None;
    }
    let bytes_per_row = if font.psf1 { 1 } else { font.width / 8 };
    let off = font.glyphs_start + c as usize * font.charsize + row * bytes_per_row;
    if off >= font_len() {
        return None;
    }
    // SAFETY: FONT_BUF holds at least font_len() validated bytes (written
    // by init()); `off` is bounds-checked above against that length.
    unsafe { Some(*(&raw const FONT_BUF as *const u8).add(off)) }
}

/// Draw one character at pixel (x, y); returns the glyph advance width.
pub fn draw_char(
    fb: &mut [u32],
    stride: usize,
    x: usize,
    y: usize,
    c: u8,
    fg: u32,
    bg: u32,
) -> usize {
    let Some(font) = current() else { return 8 };
    let bytes_per_row = if font.psf1 { 1 } else { font.width / 8 };
    for row in 0..font.height {
        let Some(row_bits) = glyph_row(&font, c, row) else {
            break;
        };
        for col in 0..font.width {
            if col >= bytes_per_row * 8 {
                break;
            }
            let on = (row_bits >> (7 - col)) & 1;
            let px = x + col;
            let py = y + row;
            if px < stride && py * stride + px < fb.len() {
                fb[py * stride + px] = if on != 0 { fg } else { bg };
            }
        }
    }
    font.width
}

/// Draw an ASCII string (stops at \n or \0); returns the x after the text.
pub fn draw_text(
    fb: &mut [u32],
    stride: usize,
    x: usize,
    y: usize,
    text: &str,
    fg: u32,
    bg: u32,
) -> usize {
    let mut cx = x;
    for &b in text.as_bytes() {
        if b == b'\n' || b == b'\0' {
            break;
        }
        cx += draw_char(fb, stride, cx, y, b, fg, bg);
    }
    cx
}

/// Draw text WITHOUT painting the background cells (transparent glyphs) —
/// for labels sitting on arbitrary backgrounds. Returns the x after the text.
pub fn draw_text_fg(
    fb: &mut [u32],
    stride: usize,
    x: usize,
    y: usize,
    text: &str,
    fg: u32,
) -> usize {
    let Some(font) = current() else { return x };
    let bytes_per_row = if font.psf1 { 1 } else { font.width / 8 };
    let mut cx = x;
    for &c in text.as_bytes() {
        if c == b'\n' || c == b'\0' {
            break;
        }
        for row in 0..font.height {
            let Some(row_bits) = glyph_row(&font, c, row) else {
                break;
            };
            for col in 0..font.width.min(bytes_per_row * 8) {
                let on = (row_bits >> (7 - col)) & 1;
                if on == 0 {
                    continue;
                }
                let px = cx + col;
                let py = y + row;
                let idx = py * stride + px;
                if px < stride && idx < fb.len() {
                    fb[idx] = fg;
                }
            }
        }
        cx += font.width;
    }
    cx
}
