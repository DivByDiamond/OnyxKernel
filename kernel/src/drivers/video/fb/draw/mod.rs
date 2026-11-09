use super::{front_base, size_bytes};

// ── Double buffering (TUI anti-flicker) ──────────────────────────────────
//
// The ANSI console (fb_term) paints every byte it receives the moment it
// arrives, so a full-screen TUI redraw (`ESC[2J` + repaint) used to show
// cleared and partially-drawn states on the visible framebuffer for tens
// of milliseconds — perceived as heavy flicker in vim/otop/oed/osnake
// (mid-redraw frames: status line missing, cursor block at an intermediate
// position). The fix: console output paints into an off-screen BACK buffer
// of identical geometry, and a debounced presenter
// (fb_term::ansi::present_tick, driven from the 100 Hz timer tick) copies
// the finished frame to the visible front buffer only after console output
// has settled.
//
// The back buffer is physically contiguous RAM (pmm::alloc_n) addressed
// through the kernel's direct physical mapping, exactly like the front
// buffer pointer. It is allocated EAGERLY by fb::init/init_device
// (single-threaded boot / set_mode context) — the presenter runs from
// timer-interrupt context and must never allocate. If the contiguous
// allocation fails, double_buffered() stays false and the console
// transparently falls back to direct-to-front painting (legacy behavior:
// fully functional, but the redraw flicker remains).

use core::sync::atomic::{AtomicUsize, Ordering};

static G_BACK_PA: AtomicUsize = AtomicUsize::new(0);
/// Pages owned by the published back-buffer run (for free on re-init).
static G_BACK_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Eagerly allocate the console back buffer for the CURRENT fb geometry.
/// Called from fb::init / fb::init_device right after the front geometry
/// is installed; safe to call again on re-init (set_mode resize path):
/// the stale run is freed first. On allocation failure the console runs
/// in direct (single-buffer) mode.
pub fn init_back_buffer() {
    let pages = size_bytes().div_ceil(crate::mm::pmm::PAGE_SIZE);
    let old_pa = G_BACK_PA.swap(0, Ordering::AcqRel) as u64;
    if old_pa != 0 {
        let old_pages = G_BACK_PAGES.swap(0, Ordering::AcqRel);
        if old_pages != 0 {
            // SAFETY: old_pa is a page-aligned pmm run this module published
            // earlier (init_back_buffer) and never republished; free
            // revalidates the range.
            unsafe { crate::mm::pmm::free(old_pa) };
        }
    }
    // SAFETY: pmm::init has completed by the time a framebuffer exists
    // (fb init runs from display/display-driver init); alloc_n self-locks
    // and zeroes the returned run, so the back buffer starts black.
    match unsafe { crate::mm::pmm::alloc_n(pages) } {
        Ok(back) => {
            G_BACK_PAGES.store(pages, Ordering::Release);
            G_BACK_PA.store(back as usize, Ordering::Release);
        }
        Err(_) => {
            crate::kwrn!(
                "fb",
                "no contiguous back buffer; console double buffering disabled"
            );
        }
    }
}

/// True when a separate back buffer exists (double-buffered console).
/// The console presenter checks this on every tick; false means legacy
/// direct-to-front painting.
#[inline]
pub fn double_buffered() -> bool {
    G_BACK_PA.load(Ordering::Acquire) != 0
}

/// Test-only hook: publish a host-provided back-buffer address so host
/// integration tests can exercise paint→present without pmm.
#[cfg(test)]
pub fn test_publish_back(pa: usize) {
    G_BACK_PA.store(pa, Ordering::Release);
    G_BACK_PAGES.store(1, Ordering::Release);
}

/// Test-only hook: drop the published back buffer (host tests).
#[cfg(test)]
pub fn test_clear_back() {
    G_BACK_PA.store(0, Ordering::Release);
    G_BACK_PAGES.store(0, Ordering::Release);
}

/// Paint-target base for console rendering: the off-screen back buffer
/// when double buffering is active, else the visible front buffer.
#[inline]
pub fn paint_base() -> *mut u8 {
    let pa = G_BACK_PA.load(Ordering::Acquire);
    if pa != 0 { pa as *mut u8 } else { front_base() }
}

/// Present the last painted frame: copy the back buffer over the visible
/// front buffer. No-op in direct (single-buffer) mode. Called by the
/// console presenter with the console write lock held, so a frame is
/// never copied mid-write.
pub fn present() {
    let Some(publication) = super::present_control::Publication::begin_console() else {
        return;
    };
    let pa = G_BACK_PA.load(Ordering::Acquire);
    if pa == 0 {
        return;
    }
    let bytes = size_bytes();
    // SAFETY: both pointers are kernel direct-mapped memory of size_bytes():
    // the front buffer is the installed framebuffer surface and the back
    // buffer is a pmm-owned contiguous run; non-overlapping by construction.
    unsafe {
        core::ptr::copy_nonoverlapping(pa as *const u8, front_base(), bytes);
    }
    publication.commit();
}

/// Publish only a console rectangle. Native applications retain full-surface
/// access; console refreshes need not copy millions of untouched MMIO pixels.
pub fn present_region(width: usize, height: usize) {
    let pa = G_BACK_PA.load(Ordering::Acquire);
    if pa == 0 {
        return;
    }
    let pitch = super::pitch();
    let row_bytes = width.min(super::width()) * (super::bpp() / 8);
    let height = height.min(super::height());
    for y in 0..height {
        // SAFETY: geometry is bounded to the installed surface; back and
        // front buffers are disjoint and both contain pitch*height bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (pa as *const u8).add(y * pitch),
                front_base().add(y * pitch),
                row_bytes,
            );
        }
    }
}

mod text;
pub use text::*;
