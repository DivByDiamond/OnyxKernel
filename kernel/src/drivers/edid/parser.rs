use onyx_core::errno::{Errno, KResult};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EdidTiming {
    pub width: u16,
    pub height: u16,
    pub refresh: u8,
    pub hsync_pol: bool,
    pub vsync_pol: bool,
    pub interlaced: bool,
}

pub struct EdidInfo {
    pub manufacturer: [u8; 2],
    pub product: u16,
    pub serial: u32,
    pub width_cm: u8,
    pub height_cm: u8,
    pub timing: [EdidTiming; 8],
    pub ntiming: usize,
    pub preferred_width: u16,
    pub preferred_height: u16,
}

fn edid_checksum(data: &[u8; 128]) -> bool {
    let mut sum: u8 = 0;
    for i in 0..128 {
        sum = sum.wrapping_add(data[i]);
    }
    sum == 0
}

unsafe fn parse_detailed_timing(block: &[u8]) -> EdidTiming {
    let w = ((block[4] as u16) >> 4) as u16 | ((block[2] as u16) << 4);
    let h = ((block[7] as u16) >> 4) as u16 | ((block[5] as u16) << 4);
    let vf = ((block[3] >> 6) as u8) | (block[7] & 0x0F);
    let vrate = if vf > 0 {
        (block[3] & 0x3F).max(60)
    } else {
        60
    };
    let interlaced = (block[3] >> 7) & 1 != 0;
    let hsync = (block[17] >> 1) & 1 != 0;
    let vsync = (block[17] >> 0) & 1 != 0;
    EdidTiming {
        width: w,
        height: h,
        refresh: vrate,
        hsync_pol: hsync,
        vsync_pol: vsync,
        interlaced,
    }
}

unsafe fn parse_established(data: &[u8; 128], timings: &mut [EdidTiming; 8], n: &mut usize) {
    let est = [data[35], data[36], data[37]];
    let tbl: &[(u8, u8, u16, u16)] = &[
        (0, 0, 800, 600),
        (0, 1, 800, 600),
        (0, 2, 640, 480),
        (0, 3, 640, 480),
        (0, 4, 640, 480),
        (0, 5, 1024, 768),
        (0, 6, 1280, 1024),
        (1, 0, 1024, 768),
        (1, 1, 1024, 768),
        (1, 2, 1280, 1024),
        (1, 3, 1152, 870),
    ];
    for &(bi, bit, w, h) in tbl {
        if *n >= 8 {
            break;
        }
        if est[bi as usize] & (1 << bit) != 0 {
            timings[*n] = EdidTiming {
                width: w,
                height: h,
                refresh: 60,
                hsync_pol: false,
                vsync_pol: false,
                interlaced: false,
            };
            *n += 1;
        }
    }
}

pub unsafe fn parse_edid(data: &[u8; 128]) -> KResult<EdidInfo> {
    if data[0] != 0x00
        || data[1] != 0xFF
        || data[2] != 0xFF
        || data[3] != 0xFF
        || data[4] != 0xFF
        || data[5] != 0xFF
        || data[6] != 0xFF
        || data[7] != 0x00
    {
        return Err(Errno::Inval);
    }
    if !edid_checksum(data) {
        return Err(Errno::Inval);
    }
    let manufacturer = [data[8] & 0x7F, data[9] & 0x7F];
    let product = (data[11] as u16) << 8 | data[10] as u16;
    let serial = (data[15] as u32) << 24
        | (data[14] as u32) << 16
        | (data[13] as u32) << 8
        | data[12] as u32;
    let width_cm = data[21];
    let height_cm = data[22];
    let mut timings: [EdidTiming; 8] = [EdidTiming {
        width: 0,
        height: 0,
        refresh: 0,
        hsync_pol: false,
        vsync_pol: false,
        interlaced: false,
    }; 8];
    let mut n = 0;
    parse_established(data, &mut timings, &mut n);
    for i in 0..8 {
        if n >= 8 {
            break;
        }
        let off = 38 + i * 2;
        let hi = data[off];
        let lo = data[off + 1];
        if hi == 0x01 && lo == 0x01 {
            continue;
        }
        if hi == 0x00 || lo == 0x00 {
            continue;
        }
        let h = (hi as u16) << 8 | lo as u16;
        let h_act = (h >> 6) + 31;
        let v_act = (h & 0x3F) + 1;
        let refresh = 60u8;
        timings[n] = EdidTiming {
            width: h_act * 8,
            height: v_act,
            refresh,
            hsync_pol: false,
            vsync_pol: false,
            interlaced: false,
        };
        n += 1;
    }
    let mut preferred_width = 0u16;
    let mut preferred_height = 0u16;
    for i in 0..4 {
        let off = 54 + i * 18;
        if off + 18 > 126 {
            break;
        }
        let tag = data[off + 3];
        if tag == 0xFF {
        } else if tag == 0xFE {
        } else if tag == 0xFC {
        } else if tag == 0xFD {
        } else if tag <= 0x0F {
            let t = parse_detailed_timing(&data[off..off + 18]);
            if i == 0 {
                preferred_width = t.width;
                preferred_height = t.height;
            }
            if n < 8 {
                timings[n] = t;
                n += 1;
            }
        }
    }
    Ok(EdidInfo {
        manufacturer,
        product,
        serial,
        width_cm,
        height_cm,
        timing: timings,
        ntiming: n,
        preferred_width,
        preferred_height,
    })
}
