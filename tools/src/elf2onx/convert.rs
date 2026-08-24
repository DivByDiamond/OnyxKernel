use super::compress;
use onyx_core::formats::{
    ONX_FLAGS_COMPRESSED, ONX_FLAGS_RING1, ONX_MAX_SEGS, OnxHeader, OnxSegment, VMM_R, VMM_W, VMM_X,
};
use std::fs::File;
use std::io::{Read, Write};
use std::process;

struct LoadInfo {
    seg: OnxSegment,
    data: Vec<u8>,
}

pub fn run(input: &str, output: &str, ring1: bool, v2: bool, do_compress: bool) {
    let mut elf_data = Vec::new();
    File::open(input)
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
    if u16::from_le_bytes([elf_data[16], elf_data[17]]) != 2 {
        eprintln!("not ET_EXEC");
        process::exit(1);
    }
    if u16::from_le_bytes([elf_data[18], elf_data[19]]) != 243 {
        eprintln!("not RISC-V");
        process::exit(1);
    }

    let e_entry = u64::from_le_bytes(elf_data[24..32].try_into().unwrap());
    let e_phoff = u64::from_le_bytes(elf_data[32..40].try_into().unwrap()) as usize;
    let e_phentsize = u16::from_le_bytes([elf_data[54], elf_data[55]]) as usize;
    let e_phnum = u16::from_le_bytes([elf_data[56], elf_data[57]]) as usize;
    let max_segs = if v2 { ONX_MAX_SEGS } else { 8 };

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

        let start = p_offset;
        let end = (p_offset + p_filesz as usize).min(elf_data.len());
        let raw = &elf_data[start..end];
        let (data, csize) = if do_compress && v2 && p_filesz > 0 {
            let c = compress::rle_compress(raw);
            let cs = c.len() as u32;
            if cs < p_filesz as u32 {
                (c, cs)
            } else {
                (raw.to_vec(), 0)
            }
        } else {
            (raw.to_vec(), 0u32)
        };
        loads.push(LoadInfo {
            seg: OnxSegment {
                vaddr: p_vaddr,
                filesz: p_filesz,
                memsz: p_memsz,
                offset: 0,
                flags,
                align: p_align as u32,
                reserved: 0,
                compressed_size: csize,
            },
            data,
        });
    }

    let nsegs = loads.len() as u32;
    let hdr_size = if v2 {
        (OnxHeader::V2_HEADER_SIZE + loads.len() * OnxSegment::SIZE_V2) as u32
    } else {
        OnxHeader::V1_HEADER_SIZE as u32
    };
    let mut data_off = hdr_size;
    for li in &mut loads {
        li.seg.offset = data_off;
        data_off = data_off.saturating_add(li.data.len() as u32);
    }

    let mut any_compressed = false;
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

    let header = OnxHeader {
        magic: onyx_core::formats::ONX_MAGIC,
        version: if v2 {
            onyx_core::formats::ONX_VERSION_2
        } else {
            onyx_core::formats::ONX_VERSION_1
        },
        entry: e_entry,
        nsegs,
        flags,
        segs: loads.iter().map(|li| li.seg).collect(),
    };

    let mut out = File::create(output).unwrap_or_else(|e| {
        eprintln!("create {}: {}", output, e);
        process::exit(1);
    });
    if v2 {
        let bytes = header.to_bytes_v2().unwrap_or_else(|e| {
            eprintln!("serialize header: {}", e.as_str());
            process::exit(1);
        });
        out.write_all(&bytes).unwrap();
    } else {
        out.write_all(&header.to_bytes_v1()).unwrap();
    }
    for li in &loads {
        out.write_all(&li.data).unwrap();
    }

    let saved: u32 = loads.iter().map(|li| li.seg.filesz as u32).sum::<u32>()
        - loads.iter().map(|li| li.data.len() as u32).sum::<u32>();
    eprintln!(
        "elf2onx: {} -> {} (v{}, entry=0x{:x}, nsegs={}, ring={}{})",
        input,
        output,
        if v2 { 2 } else { 1 },
        e_entry,
        nsegs,
        if ring1 { 1 } else { 2 },
        if any_compressed {
            format!(", compressed saved={}B", saved)
        } else {
            String::new()
        }
    );
}
