use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::process;

const ONX_MAGIC: u32 = 0x31584E4F;
const ONX_VERSION_1: u32 = 1;
const ONX_VERSION_2: u32 = 2;
const ONX_MAX_SEGS_V1: usize = 8;
const ONX_MAX_SEGS_V2: usize = 256;
const ONX_FLAGS_RING1: u32 = 0x2;
const ONX_FLAGS_COMPRESSED: u32 = 0x4;
const VMM_R: u32 = 1 << 1;
const VMM_W: u32 = 1 << 2;
const VMM_X: u32 = 1 << 3;

const V1_FIXED_HDR: usize = 24;
const V1_SEG_SIZE: usize = 40;
const V1_HEADER_SIZE: usize = V1_FIXED_HDR + ONX_MAX_SEGS_V1 * V1_SEG_SIZE;

const V2_FIXED_HDR: usize = 32;
const V2_SEG_SIZE: usize = 48;

#[repr(C, packed)]
#[derive(Default, Clone, Copy)]
struct OnxSegment {
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    offset: u32,
    flags: u32,
    align: u32,
    reserved: u32,
    compressed_size: u32,
}

impl OnxSegment {
    fn to_bytes_v1(self) -> [u8; V1_SEG_SIZE] {
        let mut b = [0u8; V1_SEG_SIZE];
        b[0..8].copy_from_slice(&self.vaddr.to_le_bytes());
        b[8..16].copy_from_slice(&self.filesz.to_le_bytes());
        b[16..24].copy_from_slice(&self.memsz.to_le_bytes());
        b[24..28].copy_from_slice(&self.offset.to_le_bytes());
        b[28..32].copy_from_slice(&self.flags.to_le_bytes());
        b[32..36].copy_from_slice(&self.align.to_le_bytes());
        b[36..40].copy_from_slice(&self.reserved.to_le_bytes());
        b
    }

    fn to_bytes_v2(self) -> [u8; V2_SEG_SIZE] {
        let mut b = [0u8; V2_SEG_SIZE];
        b[0..8].copy_from_slice(&self.vaddr.to_le_bytes());
        b[8..16].copy_from_slice(&self.filesz.to_le_bytes());
        b[16..24].copy_from_slice(&self.memsz.to_le_bytes());
        b[24..28].copy_from_slice(&self.offset.to_le_bytes());
        b[28..32].copy_from_slice(&self.flags.to_le_bytes());
        b[32..36].copy_from_slice(&self.align.to_le_bytes());
        b[36..40].copy_from_slice(&self.reserved.to_le_bytes());
        b[40..44].copy_from_slice(&self.compressed_size.to_le_bytes());
        b
    }
}

fn rle_compress(src: &[u8]) -> Vec<u8> {
    let max_dst = src.len() + src.len() / 128 + 2;
    let mut dst = vec![0u8; max_dst];
    let n = src.len();
    let mut i = 0usize;
    let mut out = 0usize;
    while i < n {
        let cur = src[i];
        let mut run = 1usize;
        while i + run < n && src[i + run] == cur && run < 128 {
            run += 1;
        }
        if run >= 3 {
            dst[out] = 0x80 | ((run - 1) as u8);
            dst[out + 1] = cur;
            out += 2;
            i += run;
        } else {
            let lit_start = i;
            let mut lit_len = 0usize;
            while i + lit_len < n && lit_len < 128 {
                let b = src[i + lit_len];
                let mut k = 0usize;
                while i + lit_len + k < n && src[i + lit_len + k] == b && k < 3 {
                    k += 1;
                }
                if k >= 3 {
                    break;
                }
                lit_len += 1;
            }
            if lit_len == 0 {
                lit_len = 1;
            }
            dst[out] = (lit_len - 1) as u8;
            for j in 0..lit_len {
                dst[out + 1 + j] = src[lit_start + j];
            }
            out += 1 + lit_len;
            i += lit_len;
        }
    }
    dst.truncate(out);
    dst
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: elf2onx [--ring=1] [--v1] [--compress] <input.elf> <output.onx>");
        process::exit(1);
    }
    let mut ring1 = false;
    let mut v1 = false;
    let mut do_compress = false;
    let mut input = String::new();
    let mut output = String::new();
    for arg in &args[1..] {
        if arg == "--ring=1" {
            ring1 = true;
        } else if arg == "--v1" {
            v1 = true;
        } else if arg == "--compress" {
            do_compress = true;
        } else if input.is_empty() {
            input = arg.clone();
        } else {
            output = arg.clone();
        }
    }
    if input.is_empty() || output.is_empty() {
        eprintln!("usage: elf2onx [--ring=1] [--v1] [--compress] <input.elf> <output.onx>");
        process::exit(1);
    }
    let v2 = !v1;

    let mut elf_data = Vec::new();
    File::open(&input)
        .unwrap_or_else(|e| {
            eprintln!("open {}: {}", input, e);
            process::exit(1);
        })
        .read_to_end(&mut elf_data)
        .unwrap_or_else(|e| {
            eprintln!("read {}: {}", input, e);
            process::exit(1);
        });

    if elf_data.len() < 64 || &elf_data[0..4] != b"\x7fELF" {
        eprintln!("not an ELF file");
        process::exit(1);
    }
    if elf_data[4] != 2 {
        eprintln!("not ELF64");
        process::exit(1);
    }
    if elf_data[5] != 1 {
        eprintln!("not little-endian");
        process::exit(1);
    }
    let e_type = u16::from_le_bytes([elf_data[16], elf_data[17]]);
    if e_type != 2 {
        eprintln!("not ET_EXEC");
        process::exit(1);
    }
    let e_machine = u16::from_le_bytes([elf_data[18], elf_data[19]]);
    if e_machine != 243 {
        eprintln!("not RISC-V");
        process::exit(1);
    }

    let e_entry = u64::from_le_bytes(elf_data[24..32].try_into().unwrap());
    let e_phoff = u64::from_le_bytes(elf_data[32..40].try_into().unwrap()) as usize;
    let e_phentsize = u16::from_le_bytes([elf_data[54], elf_data[55]]) as usize;
    let e_phnum = u16::from_le_bytes([elf_data[56], elf_data[57]]) as usize;

    let max_segs = if v2 { ONX_MAX_SEGS_V2 } else { ONX_MAX_SEGS_V1 };

    struct LoadInfo {
        seg: OnxSegment,
        data: Vec<u8>,
    }
    let mut loads: Vec<LoadInfo> = Vec::with_capacity(max_segs);

    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        if off + 56 > elf_data.len() {
            break;
        }
        let p_type = u32::from_le_bytes([
            elf_data[off],
            elf_data[off + 1],
            elf_data[off + 2],
            elf_data[off + 3],
        ]);
        if p_type != 1 {
            continue;
        }
        if loads.len() >= max_segs {
            break;
        }
        let p_flags = u32::from_le_bytes(elf_data[off + 4..off + 8].try_into().unwrap());
        let p_vaddr = u64::from_le_bytes(elf_data[off + 16..off + 24].try_into().unwrap());
        let p_filesz = u64::from_le_bytes(elf_data[off + 32..off + 40].try_into().unwrap());
        let p_memsz = u64::from_le_bytes(elf_data[off + 40..off + 48].try_into().unwrap());
        let p_align = u64::from_le_bytes(elf_data[off + 48..off + 56].try_into().unwrap());
        let p_offset = u64::from_le_bytes(elf_data[off + 8..off + 16].try_into().unwrap()) as usize;
        let mut flags = 0u32;
        if p_flags & 4 != 0 {
            flags |= VMM_R;
        }
        if p_flags & 2 != 0 {
            flags |= VMM_W;
        }
        if p_flags & 1 != 0 {
            flags |= VMM_X;
        }

        let raw_data = if do_compress && v2 && p_filesz > 0 {
            let start = p_offset;
            let end = (p_offset + p_filesz as usize).min(elf_data.len());
            let slice = &elf_data[start..end];
            let compressed = rle_compress(slice);
            let compressed_size = compressed.len() as u32;
            if compressed_size < p_filesz as u32 {
                Some((compressed, compressed_size))
            } else {
                None
            }
        } else {
            None
        };

        let (seg_data, compressed_size) = if let Some((cdata, csize)) = raw_data {
            (cdata, csize)
        } else {
            let start = p_offset;
            let end = (p_offset + p_filesz as usize).min(elf_data.len());
            (elf_data[start..end].to_vec(), 0u32)
        };

        let seg = OnxSegment {
            vaddr: p_vaddr,
            filesz: p_filesz,
            memsz: p_memsz,
            offset: 0,
            flags,
            align: p_align as u32,
            reserved: 0,
            compressed_size,
        };
        loads.push(LoadInfo {
            seg,
            data: seg_data,
        });
    }

    let nsegs = loads.len() as u32;

    // Compute header size (v2: 32 + nsegs * 48, v1: 344)
    let hdr_size: u32 = if v2 {
        (V2_FIXED_HDR + nsegs as usize * V2_SEG_SIZE) as u32
    } else {
        V1_HEADER_SIZE as u32
    };

    // Assign offsets based on actual (possibly compressed) data size.
    let mut data_offset: u32 = hdr_size;
    for li in &mut loads {
        li.seg.offset = data_offset;
        data_offset = data_offset.saturating_add(li.data.len() as u32);
    }

    let mut out = File::create(&output).unwrap_or_else(|e| {
        eprintln!("create {}: {}", output, e);
        process::exit(1);
    });

    let mut any_compressed = false;

    if v2 {
        let mut hdr = [0u8; V2_FIXED_HDR];
        hdr[0..4].copy_from_slice(&ONX_MAGIC.to_le_bytes());
        hdr[4..8].copy_from_slice(&ONX_VERSION_2.to_le_bytes());
        hdr[8..16].copy_from_slice(&e_entry.to_le_bytes());
        hdr[16..20].copy_from_slice(&nsegs.to_le_bytes());
        let mut flags = if ring1 { ONX_FLAGS_RING1 } else { 0 };
        for li in &loads {
            if li.seg.compressed_size > 0 {
                any_compressed = true;
                break;
            }
        }
        if any_compressed {
            flags |= ONX_FLAGS_COMPRESSED;
        }
        hdr[20..24].copy_from_slice(&flags.to_le_bytes());
        out.write_all(&hdr).unwrap();
        for li in &loads {
            out.write_all(&li.seg.to_bytes_v2()).unwrap();
        }
    } else {
        let mut hdr = [0u8; V1_HEADER_SIZE];
        hdr[0..4].copy_from_slice(&ONX_MAGIC.to_le_bytes());
        hdr[4..8].copy_from_slice(&ONX_VERSION_1.to_le_bytes());
        hdr[8..16].copy_from_slice(&e_entry.to_le_bytes());
        hdr[16..20].copy_from_slice(&nsegs.to_le_bytes());
        let flags = if ring1 { ONX_FLAGS_RING1 } else { 0 };
        hdr[20..24].copy_from_slice(&flags.to_le_bytes());
        for (i, li) in loads.iter().enumerate() {
            let off = V1_FIXED_HDR + i * V1_SEG_SIZE;
            hdr[off..off + V1_SEG_SIZE].copy_from_slice(&li.seg.to_bytes_v1());
        }
        out.write_all(&hdr).unwrap();
    }

    for li in &loads {
        out.write_all(&li.data).unwrap();
    }

    eprintln!(
        "elf2onx: {} -> {} (v{}, entry=0x{:x}, nsegs={}, ring={}{})",
        input,
        output,
        if v2 { 2 } else { 1 },
        e_entry,
        nsegs,
        if ring1 { 1 } else { 2 },
        if any_compressed {
            let saved: u32 = loads.iter().map(|li| li.seg.filesz as u32).sum::<u32>()
                - loads.iter().map(|li| li.data.len() as u32).sum::<u32>();
            format!(", compressed saved={}B", saved)
        } else {
            String::new()
        }
    );
}
