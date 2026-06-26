use super::compress;
use std::fs::File;
use std::io::{Read, Write};
use std::process;

const ONX_MAGIC: u32 = 0x31584E4F;
const ONX_VERSION_1: u32 = 1;
const ONX_VERSION_2: u32 = 2;
const ONX_FLAGS_RING1: u32 = 0x2;
const ONX_FLAGS_COMPRESSED: u32 = 0x4;
const V1_FIXED_HDR: usize = 24;
const V1_SEG_SIZE: usize = 40;
const V1_HEADER_SIZE: usize = V1_FIXED_HDR + 8 * V1_SEG_SIZE;
const V2_FIXED_HDR: usize = 32;
const V2_SEG_SIZE: usize = 48;
const VMM_R: u32 = 1 << 1;
const VMM_W: u32 = 1 << 2;
const VMM_X: u32 = 1 << 3;

#[derive(Clone, Copy)]
struct OnxSegment {
    vaddr: u64, filesz: u64, memsz: u64, offset: u32,
    flags: u32, align: u32, reserved: u32, compressed_size: u32,
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

struct LoadInfo { seg: OnxSegment, data: Vec<u8> }

pub fn run(input: &str, output: &str, ring1: bool, v2: bool, do_compress: bool) {
    let mut elf_data = Vec::new();
    File::open(input).unwrap_or_else(|e| { eprintln!("open {}: {}", input, e); process::exit(1); })
        .read_to_end(&mut elf_data)
        .unwrap_or_else(|e| { eprintln!("read {}: {}", input, e); process::exit(1); });

    if elf_data.len() < 64 || &elf_data[0..4] != b"\x7fELF" { eprintln!("not an ELF file"); process::exit(1); }
    if elf_data[4] != 2 { eprintln!("not ELF64"); process::exit(1); }
    if elf_data[5] != 1 { eprintln!("not little-endian"); process::exit(1); }
    if u16::from_le_bytes([elf_data[16], elf_data[17]]) != 2 { eprintln!("not ET_EXEC"); process::exit(1); }
    if u16::from_le_bytes([elf_data[18], elf_data[19]]) != 243 { eprintln!("not RISC-V"); process::exit(1); }

    let e_entry = u64::from_le_bytes(elf_data[24..32].try_into().unwrap());
    let e_phoff = u64::from_le_bytes(elf_data[32..40].try_into().unwrap()) as usize;
    let e_phentsize = u16::from_le_bytes([elf_data[54], elf_data[55]]) as usize;
    let e_phnum = u16::from_le_bytes([elf_data[56], elf_data[57]]) as usize;
    let max_segs = if v2 { 256 } else { 8 };

    let mut loads: Vec<LoadInfo> = Vec::with_capacity(max_segs);
    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        if off + 56 > elf_data.len() { break; }
        let p_type = u32::from_le_bytes([elf_data[off], elf_data[off+1], elf_data[off+2], elf_data[off+3]]);
        if p_type != 1 { continue; }
        if loads.len() >= max_segs { break; }
        let p_flags = u32::from_le_bytes(elf_data[off+4..off+8].try_into().unwrap());
        let p_vaddr = u64::from_le_bytes(elf_data[off+16..off+24].try_into().unwrap());
        let p_filesz = u64::from_le_bytes(elf_data[off+32..off+40].try_into().unwrap());
        let p_memsz = u64::from_le_bytes(elf_data[off+40..off+48].try_into().unwrap());
        let p_align = u64::from_le_bytes(elf_data[off+48..off+56].try_into().unwrap());
        let p_offset = u64::from_le_bytes(elf_data[off+8..off+16].try_into().unwrap()) as usize;
        let mut flags = 0u32;
        if p_flags & 4 != 0 { flags |= VMM_R; }
        if p_flags & 2 != 0 { flags |= VMM_W; }
        if p_flags & 1 != 0 { flags |= VMM_X; }

        let start = p_offset;
        let end = (p_offset + p_filesz as usize).min(elf_data.len());
        let raw = &elf_data[start..end];
        let (data, csize) = if do_compress && v2 && p_filesz > 0 {
            let c = compress::rle_compress(raw);
            let cs = c.len() as u32;
            if cs < p_filesz as u32 { (c, cs) } else { (raw.to_vec(), 0) }
        } else { (raw.to_vec(), 0u32) };
        loads.push(LoadInfo {
            seg: OnxSegment { vaddr: p_vaddr, filesz: p_filesz, memsz: p_memsz,
                offset: 0, flags, align: p_align as u32, reserved: 0, compressed_size: csize },
            data,
        });
    }

    let nsegs = loads.len() as u32;
    let hdr_size = if v2 { (V2_FIXED_HDR + nsegs as usize * V2_SEG_SIZE) as u32 } else { V1_HEADER_SIZE as u32 };
    let mut data_off = hdr_size;
    for li in &mut loads { li.seg.offset = data_off; data_off = data_off.saturating_add(li.data.len() as u32); }

    let mut out = File::create(output).unwrap_or_else(|e| { eprintln!("create {}: {}", output, e); process::exit(1); });
    let mut any_compressed = false;
    if v2 {
        let mut hdr = [0u8; V2_FIXED_HDR];
        hdr[0..4].copy_from_slice(&ONX_MAGIC.to_le_bytes());
        hdr[4..8].copy_from_slice(&ONX_VERSION_2.to_le_bytes());
        hdr[8..16].copy_from_slice(&e_entry.to_le_bytes());
        hdr[16..20].copy_from_slice(&nsegs.to_le_bytes());
        let mut flags = if ring1 { ONX_FLAGS_RING1 } else { 0 };
        for li in &loads { if li.seg.compressed_size > 0 { any_compressed = true; break; } }
        if any_compressed { flags |= ONX_FLAGS_COMPRESSED; }
        hdr[20..24].copy_from_slice(&flags.to_le_bytes());
        out.write_all(&hdr).unwrap();
        for li in &loads { out.write_all(&li.seg.to_bytes_v2()).unwrap(); }
    } else {
        let mut hdr = [0u8; V1_HEADER_SIZE];
        hdr[0..4].copy_from_slice(&ONX_MAGIC.to_le_bytes());
        hdr[4..8].copy_from_slice(&ONX_VERSION_1.to_le_bytes());
        hdr[8..16].copy_from_slice(&e_entry.to_le_bytes());
        hdr[16..20].copy_from_slice(&nsegs.to_le_bytes());
        hdr[20..24].copy_from_slice(&if ring1 { ONX_FLAGS_RING1 } else { 0 }.to_le_bytes());
        for (i, li) in loads.iter().enumerate() {
            let off = V1_FIXED_HDR + i * V1_SEG_SIZE;
            hdr[off..off+V1_SEG_SIZE].copy_from_slice(&li.seg.to_bytes_v1());
        }
        out.write_all(&hdr).unwrap();
    }
    for li in &loads { out.write_all(&li.data).unwrap(); }

    let saved: u32 = loads.iter().map(|li| li.seg.filesz as u32).sum::<u32>()
        - loads.iter().map(|li| li.data.len() as u32).sum::<u32>();
    eprintln!("elf2onx: {} -> {} (v{}, entry=0x{:x}, nsegs={}, ring={}{})",
        input, output, if v2 { 2 } else { 1 }, e_entry, nsegs,
        if ring1 { 1 } else { 2 },
        if any_compressed { format!(", compressed saved={}B", saved) } else { String::new() });
}
