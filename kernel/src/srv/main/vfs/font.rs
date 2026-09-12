//! Console font loading from the mounted root filesystem.

use crate::fs::vfs;
use crate::mm::heap;
use onyx_core::errno::KResult;

/// # Safety
///
/// Reads /font/default.psf into a heap buffer and hands it to font::init.
/// The buffer is intentionally NOT freed: font::init retains raw pointers
/// into it (font/psf2.rs stores `glyphs`/`unicode` into G_FONT), so the
/// blob must outlive the kernel. Leak is bounded by the font file size.
pub(crate) unsafe fn load_font() {
    // SAFETY: from_raw_parts covers exactly the kmalloc'd `size` bytes; the
    // buffer is leaked on purpose because font::init retains raw pointers
    // into it, so freeing it would leave every glyph read dangling.
    unsafe {
        (|| -> KResult<()> {
            let token = vfs::open(b"/font/default.psf", vfs::PERM_READ)?;
            let mut size = 0u32;
            vfs::stat(token, &mut size).ok();
            if size > 0 {
                let buf = heap::kmalloc(size as usize)?;
                vfs::read(token, buf, size).ok();
                vfs::close(token).ok();
                // Leak `buf` on purpose: font::init kept raw pointers into
                // the PSF blob (G_FONT.glyphs/unicode). Freeing it would
                // leave every glyph read dangling (UAF on the console path).
                match crate::font::init(core::slice::from_raw_parts(buf, size as usize)) {
                    Ok(()) => crate::kinf!("font", "loaded /font/default.psf"),
                    // Root cause (blank-console investigation, 2026-09-12): this
                    // used to be `.ok()`, so a header that parses but fails the
                    // glyph-area bounds check still logged "loaded" and the
                    // console silently painted every glyph blank.
                    Err(_) => crate::kwrn!(
                        "font",
                        "/font/default.psf failed to parse, using blank font"
                    ),
                }
            } else {
                vfs::close(token).ok();
            }
            Ok(())
        })()
        .unwrap_or_else(|_| crate::kwrn!("font", "no /font/default.psf, using blank font"));
    }
}
