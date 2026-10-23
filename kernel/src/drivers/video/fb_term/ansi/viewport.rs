//! Console-only aspect-fit presentation. Native framebuffer geometry is untouched.
//! GPL-3.0-or-later, matching OnyxKernel.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect { pub x0: usize, pub y0: usize, pub x1: usize, pub y1: usize }
impl Rect {
    pub const fn new(x0: usize, y0: usize, x1: usize, y1: usize) -> Self { Self { x0, y0, x1, y1 } }
    pub fn clipped(self, width: usize, height: usize) -> Self { Self::new(self.x0.min(width), self.y0.min(height), self.x1.min(width), self.y1.min(height)) }
    pub fn empty(self) -> bool { self.x0 >= self.x1 || self.y0 >= self.y1 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub logical_width: usize, pub logical_height: usize,
    pub width: usize, pub height: usize, pub x: usize, pub y: usize,
}
impl Viewport {
    pub fn fit(physical_width: usize, physical_height: usize, logical_width: usize, logical_height: usize) -> Option<Self> {
        if physical_width == 0 || physical_height == 0 || logical_width == 0 || logical_height == 0 { return None; }
        let (width, height) = if physical_width as u64 * logical_height as u64 <= physical_height as u64 * logical_width as u64 {
            (physical_width, (logical_height as u64 * physical_width as u64 / logical_width as u64) as usize)
        } else { ((logical_width as u64 * physical_height as u64 / logical_height as u64) as usize, physical_height) };
        if width == 0 || height == 0 { return None; }
        Some(Self { logical_width, logical_height, width, height, x: (physical_width - width) / 2, y: (physical_height - height) / 2 })
    }
    /// Exact destination coverage of a logical rectangle under floor-based nearest sampling.
    pub fn mapped(self, rect: Rect) -> Rect {
        let r = rect.clipped(self.logical_width, self.logical_height);
        let ceil = |n: usize, scale: usize, denom: usize| (n as u64 * scale as u64).div_ceil(denom as u64) as usize;
        Rect::new(self.x + ceil(r.x0, self.width, self.logical_width), self.y + ceil(r.y0, self.height, self.logical_height), self.x + ceil(r.x1, self.width, self.logical_width), self.y + ceil(r.y1, self.height, self.logical_height))
    }
}

/// Present only the changed logical rectangle. Source retains the physical pitch.
/// No allocation, libc copy, frame-wide scan, or per-pixel division occurs here.
///
/// # Safety
/// Source and destination must be distinct live surfaces covering their validated
/// pitches and heights; bpp must be 16 or 32. Caller serializes console paint/present.
/// Front may be MMIO; every destination store is volatile. Source reads target RAM.
pub unsafe fn present_rect(view: Viewport, rect: Rect, source: *const u8, destination: *mut u8, source_pitch: usize, destination_pitch: usize, bpp: usize) -> usize {
    let physical = view.mapped(rect);
    if physical.empty() || (bpp != 16 && bpp != 32) { return 0; }
    let dx0 = physical.x0 - view.x;
    let dy0 = physical.y0 - view.y;
    let mut sy = dy0 * view.logical_height / view.height;
    let mut y_remainder = dy0 * view.logical_height % view.height;
    let bytes = bpp / 8;
    for y in physical.y0..physical.y1 {
        let mut sx = dx0 * view.logical_width / view.width;
        let mut x_remainder = dx0 * view.logical_width % view.width;
        for x in physical.x0..physical.x1 {
            // SAFETY: nearest coordinates lie within the clipped logical region;
            // caller provided valid surfaces/pitches and aligned 16/32-bit pixels.
            unsafe {
                let src = source.add(sy * source_pitch + sx * bytes);
                let dst = destination.add(y * destination_pitch + x * bytes);
                if bpp == 16 { core::ptr::write_volatile(dst.cast::<u16>(), core::ptr::read(src.cast::<u16>())); }
                else { core::ptr::write_volatile(dst.cast::<u32>(), core::ptr::read(src.cast::<u32>())); }
            }
            x_remainder += view.logical_width;
            while x_remainder >= view.width { x_remainder -= view.width; sx += 1; }
        }
        y_remainder += view.logical_height;
        while y_remainder >= view.height { y_remainder -= view.height; sy += 1; }
    }
    (physical.x1 - physical.x0) * (physical.y1 - physical.y0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn high_resolution_fills_height_without_stretch() {
        let v=Viewport::fit(1920,1080,640,384).unwrap();
        assert_eq!((v.x,v.y,v.width,v.height),(60,0,1800,1080));
        assert_eq!(v.mapped(Rect::new(0,0,640,384)),Rect::new(60,0,1860,1080));
    }
    #[test] fn native_and_small_framebuffers_keep_all_cells() {
        let v=Viewport::fit(640,480,640,384).unwrap();assert_eq!((v.x,v.y,v.width,v.height),(0,48,640,384));
        let v=Viewport::fit(320,200,320,192).unwrap();assert_eq!((v.x,v.y,v.width,v.height),(0,4,320,192));
        assert_eq!(Viewport::fit(0,200,640,384),None);
    }
    #[test] fn tui_footer_and_rightmost_cell_are_inside_every_display() {
        for (w,h,cols,rows) in [(1920,1080,80,24),(640,480,80,24),(320,200,40,12)] {
            let v=Viewport::fit(w,h,cols*8,rows*16).unwrap();
            let r=v.mapped(Rect::new((cols-1)*8,(rows-1)*16,cols*8,rows*16));
            assert!(!r.empty());assert!(r.x1<=w&&r.y1<=h);assert!(r.x0<v.x+v.width&&r.y0<v.y+v.height);
        }
    }
    #[test] fn real_rgb565_pixels_scale_exactly_and_dirty_glyph_preserves_neighbors() {
        let (w,h)=(1920,1080);let v=Viewport::fit(w,h,640,384).unwrap();
        let mut back=vec![0u16;w*h];let mut front=vec![0x1234u16;w*h];
        let glyph=Rect::new(79*8,23*16,80*8,24*16);
        for y in glyph.y0..glyph.y1 {for x in glyph.x0..glyph.x1 {back[y*w+x]=if (x+y)%2==0 {0xf800}else{0x07e0};}}
        let count=unsafe{present_rect(v,glyph,back.as_ptr().cast(),front.as_mut_ptr().cast(),w*2,w*2,16)};
        let mapped=v.mapped(glyph);assert_eq!(count,(mapped.x1-mapped.x0)*(mapped.y1-mapped.y0));assert!(count<1100,"one cell must not copy the full HD surface");
        for y in 0..h {for x in 0..w {let expected=if x>=mapped.x0&&x<mapped.x1&&y>=mapped.y0&&y<mapped.y1 {let sx=(x-v.x)*v.logical_width/v.width;let sy=(y-v.y)*v.logical_height/v.height;back[sy*w+sx]}else{0x1234};assert_eq!(front[y*w+x],expected,"pixel {x},{y}");}}
    }
    #[test] fn rgb8888_nearest_sampling_matches_at_all_positions_with_padded_stride() {
        let (w,h,pitch)=(640,480,648);let v=Viewport::fit(w,h,640,384).unwrap();
        let back:Vec<u32>=(0..pitch*h).map(|i|i as u32).collect();let mut front=vec![u32::MAX;pitch*h];
        let changed=Rect::new(0,0,640,384);unsafe{present_rect(v,changed,back.as_ptr().cast(),front.as_mut_ptr().cast(),pitch*4,pitch*4,32);}
        for y in 0..h {for x in 0..pitch {let expected=if y>=48&&y<432&&x<640 {back[(y-48)*pitch+x]}else{u32::MAX};assert_eq!(front[y*pitch+x],expected);}}
    }
    #[test] fn fractional_dirty_rect_partition_has_no_gaps_or_double_coverage() {
        let v=Viewport::fit(1920,1080,640,384).unwrap();for x in 0..639 {let left=v.mapped(Rect::new(x,0,x+1,1));let right=v.mapped(Rect::new(x+1,0,x+2,1));assert_eq!(left.x1,right.x0);}
    }
}
