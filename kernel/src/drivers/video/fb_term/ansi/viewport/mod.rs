//! Console-only aspect-fit presentation. Native framebuffer geometry is untouched.
//! GPL-3.0-or-later, matching OnyxKernel.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}
impl Rect {
    pub const fn new(x0: usize, y0: usize, x1: usize, y1: usize) -> Self {
        Self { x0, y0, x1, y1 }
    }
    pub fn clipped(self, width: usize, height: usize) -> Self {
        Self::new(
            self.x0.min(width),
            self.y0.min(height),
            self.x1.min(width),
            self.y1.min(height),
        )
    }
    pub fn empty(self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub logical_width: usize,
    pub logical_height: usize,
    pub width: usize,
    pub height: usize,
    pub x: usize,
    pub y: usize,
}
impl Viewport {
    pub fn fit(
        physical_width: usize,
        physical_height: usize,
        logical_width: usize,
        logical_height: usize,
    ) -> Option<Self> {
        if physical_width == 0 || physical_height == 0 || logical_width == 0 || logical_height == 0
        {
            return None;
        }
        let (width, height) = if physical_width as u64 * logical_height as u64
            <= physical_height as u64 * logical_width as u64
        {
            (
                physical_width,
                (logical_height as u64 * physical_width as u64 / logical_width as u64) as usize,
            )
        } else {
            (
                (logical_width as u64 * physical_height as u64 / logical_height as u64) as usize,
                physical_height,
            )
        };
        if width == 0 || height == 0 {
            return None;
        }
        Some(Self {
            logical_width,
            logical_height,
            width,
            height,
            x: (physical_width - width) / 2,
            y: (physical_height - height) / 2,
        })
    }
    /// Exact destination coverage of a logical rectangle under floor-based nearest sampling.
    pub fn mapped(self, rect: Rect) -> Rect {
        let r = rect.clipped(self.logical_width, self.logical_height);
        let ceil = |n: usize, scale: usize, denom: usize| {
            (n as u64 * scale as u64).div_ceil(denom as u64) as usize
        };
        Rect::new(
            self.x + ceil(r.x0, self.width, self.logical_width),
            self.y + ceil(r.y0, self.height, self.logical_height),
            self.x + ceil(r.x1, self.width, self.logical_width),
            self.y + ceil(r.y1, self.height, self.logical_height),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentStats {
    pub pixels: usize,
    pub stores: usize,
}

/// Present only the changed logical rectangle. Source retains the physical pitch.
/// No allocation, libc copy, frame-wide scan, or per-pixel division occurs here.
///
/// # Safety
/// Source and destination must be distinct live surfaces covering their validated
/// pitches and heights, with 16/32-bit aligned pixels and pitches; bpp must be 16 or 32. Caller serializes console paint/present.
/// Front may be MMIO supporting aligned native pixel and (on RV64) 64-bit stores;
/// every destination store is volatile. Source reads target RAM.
pub unsafe fn present_rect(
    view: Viewport,
    rect: Rect,
    source: *const u8,
    destination: *mut u8,
    source_pitch: usize,
    destination_pitch: usize,
    bpp: usize,
) -> PresentStats {
    match bpp {
        // SAFETY: the caller supplies valid aligned, disjoint surfaces.
        16 => unsafe {
            present_pixels::<2>(
                view,
                rect,
                source,
                destination,
                source_pitch,
                destination_pitch,
            )
        },
        32 => unsafe {
            present_pixels::<4>(
                view,
                rect,
                source,
                destination,
                source_pitch,
                destination_pitch,
            )
        },
        _ => PresentStats::default(),
    }
}

unsafe fn present_pixels<const BYTES: usize>(
    view: Viewport,
    rect: Rect,
    source: *const u8,
    destination: *mut u8,
    source_pitch: usize,
    destination_pitch: usize,
) -> PresentStats {
    let physical = view.mapped(rect);
    if physical.empty() {
        return PresentStats::default();
    }
    // Reuse the nearest-neighbour x walk across every output row. A bounded
    // 1 KiB workspace avoids allocation/large interrupt stacks and works even
    // when framebuffer width exceeds the workspace. Chunks never overlap.
    let mut offsets = [0usize; 128];
    let dx0 = physical.x0 - view.x;
    let dy0 = physical.y0 - view.y;
    let mut sx = dx0 * view.logical_width / view.width;
    let mut remainder = dx0 * view.logical_width % view.width;
    let mut stores = 0;
    let mut left = physical.x0;
    while left < physical.x1 {
        let count = (physical.x1 - left).min(offsets.len());
        for offset in &mut offsets[..count] {
            *offset = sx * BYTES;
            remainder += view.logical_width;
            while remainder >= view.width {
                remainder -= view.width;
                sx += 1;
            }
        }
        let mut sy = dy0 * view.logical_height / view.height;
        let mut y_remainder = dy0 * view.logical_height % view.height;
        for y in physical.y0..physical.y1 {
            let row = unsafe { source.add(sy * source_pitch) };
            let mut dst = unsafe { destination.add(y * destination_pitch + left * BYTES) };
            let mut i = 0;
            while i < count {
                let packed = cfg!(target_pointer_width = "64")
                    && (dst as usize).is_multiple_of(8)
                    && count - i >= 8 / BYTES;
                // SAFETY: i + group remains within the initialized offsets;
                // row offsets sample the clipped logical surface. Destination
                // stores stay inside this dirty row, with scalar aligned edges.
                unsafe {
                    let read = |j: usize| {
                        let ptr = row.add(*offsets.get_unchecked(j));
                        if BYTES == 2 {
                            ptr.cast::<u16>().read() as u64
                        } else {
                            ptr.cast::<u32>().read() as u64
                        }
                    };
                    let first = read(i);
                    if packed {
                        let second = read(i + 1);
                        let word = if BYTES == 2 {
                            let third = read(i + 2);
                            let fourth = read(i + 3);
                            if cfg!(target_endian = "little") {
                                first | second << 16 | third << 32 | fourth << 48
                            } else {
                                first << 48 | second << 32 | third << 16 | fourth
                            }
                        } else if cfg!(target_endian = "little") {
                            first | second << 32
                        } else {
                            first << 32 | second
                        };
                        core::ptr::write_volatile(dst.cast::<u64>(), word);
                        i += 8 / BYTES;
                        dst = dst.add(8);
                    } else {
                        if BYTES == 2 {
                            core::ptr::write_volatile(dst.cast::<u16>(), first as u16);
                        } else {
                            core::ptr::write_volatile(dst.cast::<u32>(), first as u32);
                        }
                        i += 1;
                        dst = dst.add(BYTES);
                    }
                }
                stores += 1;
            }
            y_remainder += view.logical_height;
            while y_remainder >= view.height {
                y_remainder -= view.height;
                sy += 1;
            }
        }
        left += count;
    }
    PresentStats {
        pixels: (physical.x1 - physical.x0) * (physical.y1 - physical.y0),
        stores,
    }
}

#[cfg(test)]
mod tests;
