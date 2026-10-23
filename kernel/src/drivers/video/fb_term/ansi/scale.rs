//! Console-only scaled presentation; native fb devices retain their full geometry.
use super::state::{AnsiTerm, FONT_H, FONT_W};
use super::viewport::{Rect, Viewport};
use crate::drivers::fb;
use core::sync::atomic::{AtomicUsize, Ordering};

static X0: AtomicUsize = AtomicUsize::new(usize::MAX);
static Y0: AtomicUsize = AtomicUsize::new(usize::MAX);
static X1: AtomicUsize = AtomicUsize::new(0);
static Y1: AtomicUsize = AtomicUsize::new(0);
// Accessed only under the existing console write/presentation lock.
static mut LAST_VIEW: Option<Viewport> = None;
static mut LAST_CURSOR: Option<Rect> = None;

pub(super) fn mark(rect: Rect) {
    if rect.empty() { return; }
    X0.fetch_min(rect.x0, Ordering::Relaxed);
    Y0.fetch_min(rect.y0, Ordering::Relaxed);
    X1.fetch_max(rect.x1, Ordering::Relaxed);
    Y1.fetch_max(rect.y1, Ordering::Relaxed);
}
fn take_dirty() -> Option<Rect> {
    let rect=Rect::new(X0.swap(usize::MAX,Ordering::Relaxed),Y0.swap(usize::MAX,Ordering::Relaxed),X1.swap(0,Ordering::Relaxed),Y1.swap(0,Ordering::Relaxed));
    if rect.empty(){None}else{Some(rect)}
}
fn cell(col:usize,row:usize)->Rect {Rect::new(col*FONT_W,row*FONT_H,(col+1)*FONT_W,(row+1)*FONT_H)}

/// Return true when the console frame and cursor were presented here.
/// SAFETY: caller holds the existing UART_LOCK, excluding paint/present.
pub(super) unsafe fn present(term:&AnsiTerm)->bool {
    if !crate::drivers::fb_term::geometry::serial_mirrored() { return false; }
    let Some(view)=Viewport::fit(fb::width(),fb::height(),term.cols*FONT_W,term.rows*FONT_H) else{return false;};
    let mut dirty=take_dirty();
    // SAFETY: only the console lock holder accesses these presentation records.
    unsafe {
        if LAST_VIEW!=Some(view) {
            dirty=Some(Rect::new(0,0,view.logical_width,view.logical_height));
            clear_margins(view);
            LAST_VIEW=Some(view);
            LAST_CURSOR=None;
        }
        let cursor=if term.cursor_visible {Some(cell(term.cur_col,term.cur_row))}else{None};
        if let Some(rect)=dirty {super::viewport::present_rect(view,rect,fb::paint_base(),fb::front_base(),fb::pitch(),fb::pitch(),fb::bpp());}
        // Restore cursor cells separately. A cursor move from the bottom-right
        // to the top-left must not expand a one-glyph change to a full frame.
        for rect in [LAST_CURSOR,cursor].into_iter().flatten() {
            super::viewport::present_rect(view,rect,fb::paint_base(),fb::front_base(),fb::pitch(),fb::pitch(),fb::bpp());
        }
        if let Some(current)=cursor {
            let color=if term.reverse {term.bg}else{term.fg};
            fill_front(view.mapped(current),color);
        }
        LAST_CURSOR=cursor;
    }
    true
}
unsafe fn clear_margins(v:Viewport) {
    // Only letterbox margins are cleared; no extra full-frame copy.
    unsafe {
        fill_front(Rect::new(0,0,fb::width(),v.y),fb::COL_BLACK);
        fill_front(Rect::new(0,v.y+v.height,fb::width(),fb::height()),fb::COL_BLACK);
        fill_front(Rect::new(0,v.y,v.x,v.y+v.height),fb::COL_BLACK);
        fill_front(Rect::new(v.x+v.width,v.y,fb::width(),v.y+v.height),fb::COL_BLACK);
    }
}
unsafe fn fill_front(rect:Rect,color:u32) {
    let bytes=fb::bpp()/8;let base=fb::front_base();let pitch=fb::pitch();
    let packed=fb::rgb32_to_r5g6b5(color);
    for y in rect.y0..rect.y1 {for x in rect.x0..rect.x1 {
        // SAFETY: rectangles are derived from the installed bounded viewport.
        unsafe {let ptr=base.add(y*pitch+x*bytes);match bytes {2=>core::ptr::write_volatile(ptr.cast::<u16>(),packed),4=>core::ptr::write_volatile(ptr.cast::<u32>(),color),_=>{}}}
    }}
}
