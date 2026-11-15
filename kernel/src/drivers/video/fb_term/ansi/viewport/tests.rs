use super::*;
#[test]
fn high_resolution_fills_height_without_stretch() {
    let v = Viewport::fit(1920, 1080, 640, 384).unwrap();
    assert_eq!((v.x, v.y, v.width, v.height), (60, 0, 1800, 1080));
    assert_eq!(
        v.mapped(Rect::new(0, 0, 640, 384)),
        Rect::new(60, 0, 1860, 1080)
    );
}
#[test]
fn native_and_small_framebuffers_keep_all_cells() {
    let v = Viewport::fit(640, 480, 640, 384).unwrap();
    assert_eq!((v.x, v.y, v.width, v.height), (0, 48, 640, 384));
    let v = Viewport::fit(320, 200, 320, 192).unwrap();
    assert_eq!((v.x, v.y, v.width, v.height), (0, 4, 320, 192));
    assert_eq!(Viewport::fit(0, 200, 640, 384), None);
}
#[test]
fn tui_footer_and_rightmost_cell_are_inside_every_display() {
    for (w, h, cols, rows) in [(1920, 1080, 80, 24), (640, 480, 80, 24), (320, 200, 40, 12)] {
        let v = Viewport::fit(w, h, cols * 8, rows * 16).unwrap();
        let r = v.mapped(Rect::new(
            (cols - 1) * 8,
            (rows - 1) * 16,
            cols * 8,
            rows * 16,
        ));
        assert!(!r.empty());
        assert!(r.x1 <= w && r.y1 <= h);
        assert!(r.x0 < v.x + v.width && r.y0 < v.y + v.height);
    }
}
#[test]
fn real_rgb565_pixels_scale_exactly_and_dirty_glyph_preserves_neighbors() {
    let (w, h) = (1920, 1080);
    let v = Viewport::fit(w, h, 640, 384).unwrap();
    let mut back = vec![0u16; w * h];
    let mut front = vec![0x1234u16; w * h];
    let glyph = Rect::new(79 * 8, 23 * 16, 80 * 8, 24 * 16);
    for y in glyph.y0..glyph.y1 {
        for x in glyph.x0..glyph.x1 {
            back[y * w + x] = if (x + y) % 2 == 0 { 0xf800 } else { 0x07e0 };
        }
    }
    let count = unsafe {
        present_rect(
            v,
            glyph,
            back.as_ptr().cast(),
            front.as_mut_ptr().cast(),
            w * 2,
            w * 2,
            16,
        )
    };
    let mapped = v.mapped(glyph);
    assert_eq!(
        count.pixels,
        (mapped.x1 - mapped.x0) * (mapped.y1 - mapped.y0)
    );
    assert!(
        count.pixels < 1100,
        "one cell must not copy the full HD surface"
    );
    for y in 0..h {
        for x in 0..w {
            let expected = if x >= mapped.x0 && x < mapped.x1 && y >= mapped.y0 && y < mapped.y1 {
                let sx = (x - v.x) * v.logical_width / v.width;
                let sy = (y - v.y) * v.logical_height / v.height;
                back[sy * w + sx]
            } else {
                0x1234
            };
            assert_eq!(front[y * w + x], expected, "pixel {x},{y}");
        }
    }
}
#[test]
fn rgb8888_nearest_sampling_matches_at_all_positions_with_padded_stride() {
    let (w, h, pitch) = (640, 480, 648);
    let v = Viewport::fit(w, h, 640, 384).unwrap();
    let back: Vec<u32> = (0..pitch * h).map(|i| i as u32).collect();
    let mut front = vec![u32::MAX; pitch * h];
    let changed = Rect::new(0, 0, 640, 384);
    unsafe {
        present_rect(
            v,
            changed,
            back.as_ptr().cast(),
            front.as_mut_ptr().cast(),
            pitch * 4,
            pitch * 4,
            32,
        );
    }
    for y in 0..h {
        for x in 0..pitch {
            let expected = if (48..432).contains(&y) && x < 640 {
                back[(y - 48) * pitch + x]
            } else {
                u32::MAX
            };
            assert_eq!(front[y * pitch + x], expected);
        }
    }
}
#[test]
fn fractional_dirty_rect_partition_has_no_gaps_or_double_coverage() {
    let v = Viewport::fit(1920, 1080, 640, 384).unwrap();
    for x in 0..639 {
        let left = v.mapped(Rect::new(x, 0, x + 1, 1));
        let right = v.mapped(Rect::new(x + 1, 0, x + 2, 1));
        assert_eq!(left.x1, right.x0);
    }
}
#[test]
fn full_oc2r_frame_uses_one_mmio_store_per_four_rgb565_pixels() {
    let v = Viewport::fit(1024, 768, 640, 384).unwrap();
    assert_eq!((v.width, v.height), (1024, 614));
    let back = vec![0xabcd1234u64; 1024 * 768 / 4];
    let mut front = vec![0u64; back.len()];
    let stats = unsafe {
        present_rect(
            v,
            Rect::new(0, 0, 640, 384),
            back.as_ptr().cast(),
            front.as_mut_ptr().cast(),
            2048,
            2048,
            16,
        )
    };
    assert_eq!(stats.pixels, 628736);
    assert_eq!(stats.stores, 157184);
    println!(
        "OC2R full frame: {} pixels, {} MMIO stores (scalar baseline: {})",
        stats.pixels, stats.stores, stats.pixels
    );
}

#[test]
fn packed_stores_match_scalar_reference_with_unaligned_edges_and_downscaling() {
    for bpp in [16, 32] {
        let bytes = bpp / 8;
        for (w, h) in [(1024, 768), (91, 59), (37, 23)] {
            let view = Viewport::fit(w, h, 80, 48).unwrap();
            let source_pitch = 87 * bytes;
            let destination_pitch = (w + 3) * bytes;
            let back: Vec<u64> = (0..(source_pitch * 48).div_ceil(8) + 1)
                .map(|x| (x as u64).wrapping_mul(0x0123456789abcdef))
                .collect();
            for offset in [0, bytes] {
                for rect in [
                    Rect::new(1, 1, 79, 47),
                    Rect::new(3, 7, 4, 8),
                    Rect::new(0, 0, 99, 99),
                    Rect::new(80, 48, 81, 49),
                ] {
                    let mut front = vec![
                        0xa5a5a5a5a5a5a5a5u64;
                        (destination_pitch * h + offset).div_ceil(8) + 1
                    ];
                    let mut expected = front.clone();
                    let r = view.mapped(rect);
                    for y in r.y0..r.y1 {
                        for x in r.x0..r.x1 {
                            let sy = (y - view.y) * view.logical_height / view.height;
                            let sx = (x - view.x) * view.logical_width / view.width;
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    back.as_ptr()
                                        .cast::<u8>()
                                        .add(sy * source_pitch + sx * bytes),
                                    expected
                                        .as_mut_ptr()
                                        .cast::<u8>()
                                        .add(offset + y * destination_pitch + x * bytes),
                                    bytes,
                                );
                            }
                        }
                    }
                    unsafe {
                        present_rect(
                            view,
                            rect,
                            back.as_ptr().cast(),
                            front.as_mut_ptr().cast::<u8>().add(offset),
                            source_pitch,
                            destination_pitch,
                            bpp,
                        );
                    }
                    assert_eq!(
                        front, expected,
                        "bpp={bpp} {w}x{h} offset={offset} rect={rect:?}"
                    );
                }
            }
        }
    }
}
