mod dirent;
mod inode;
mod superblock;
mod tree;

use std::env;
use std::fs::File;
use std::io::Write;
use std::process;

const ONYFS_BLOCK_SIZE: usize = 4096;
const SNAPSHOT_BLOCKS_EACH: u32 = 64;
const MAX_SNAPSHOTS: u32 = 4;
const JOURNAL_BLOCKS: u32 = 32;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: mkimage [--v1] [<manifest>] <output.img> [--add ...] [--add-dir ...]");
        process::exit(1);
    }
    let mut v1 = false;
    let mut arg_idx = 1;
    if arg_idx < args.len() && args[arg_idx] == "--v1" {
        v1 = true;
        arg_idx += 1;
    }

    let inode_size = if v1 { 64 } else { 128 };
    let inodes_per_block = ONYFS_BLOCK_SIZE / inode_size;

    let mut dirs: Vec<tree::DirNode> = Vec::new();
    let mut files: Vec<tree::Entry> = Vec::new();
    let mut next_ino: u32 = 2;
    dirs.push(tree::DirNode::root());

    let mut i = arg_idx;
    if i < args.len() && !args[i].starts_with("--") {
        let manifest_path = &args[i];
        i += 1;
        let manifest = std::fs::read_to_string(manifest_path).unwrap_or_else(|e| {
            eprintln!("read manifest {}: {}", manifest_path, e);
            process::exit(1);
        });
        for line in manifest.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            match parts[0] {
                "dir" => tree::add_dir(&mut dirs, parts.get(1).unwrap_or(&"/"), &mut next_ino),
                "file" => {
                    let local = parts.get(1).unwrap_or(&"");
                    let fs_path = parts
                        .get(2)
                        .unwrap_or(&"")
                        .split_whitespace()
                        .next()
                        .unwrap_or("");
                    tree::add_file(&mut dirs, &mut files, local, fs_path, &mut next_ino);
                }
                _ => eprintln!("warning: unknown manifest entry: {}", line),
            }
        }
    }

    let output_path;
    loop {
        if i >= args.len() {
            eprintln!("missing output path");
            process::exit(1);
        }
        match args[i].as_str() {
            "--add" => {
                i += 1;
                if i + 1 >= args.len() {
                    eprintln!("--add requires <host> <fs_path>");
                    process::exit(1);
                }
                let host = &args[i];
                i += 1;
                let fs_path = &args[i];
                i += 1;
                tree::add_file(&mut dirs, &mut files, host, fs_path, &mut next_ino);
            }
            "--add-dir" => {
                i += 1;
                if i + 1 >= args.len() {
                    eprintln!("--add-dir requires <host_dir> <fs_prefix>");
                    process::exit(1);
                }
                let host_dir = &args[i];
                i += 1;
                let fs_prefix = &args[i];
                i += 1;
                tree::add_dir_recursive(&mut dirs, &mut files, host_dir, fs_prefix, &mut next_ino);
            }
            _ => {
                output_path = args[i].clone();
                i += 1;
                break;
            }
        }
    }
    if i < args.len() {
        eprintln!("warning: extra arguments ignored: {:?}", &args[i..]);
    }
    if files.is_empty() && dirs.len() == 1 {
        eprintln!("warning: no files or directories added to image");
    }

    let total_inodes = next_ino;
    let inode_table_blocks = (total_inodes as usize).div_ceil(inodes_per_block) as u32;
    let mut data_blocks_needed: u32 = 0;
    for _d in &dirs {
        data_blocks_needed += 1;
    }
    for f in &files {
        data_blocks_needed += f.data.len().div_ceil(ONYFS_BLOCK_SIZE) as u32;
    }

    let snapshot_blocks = if !v1 {
        MAX_SNAPSHOTS * SNAPSHOT_BLOCKS_EACH
    } else {
        0
    };
    let journal_blocks = if !v1 { JOURNAL_BLOCKS } else { 0 };
    let metadata_blocks = 3 + inode_table_blocks + snapshot_blocks + journal_blocks;
    let total_blocks = metadata_blocks + data_blocks_needed;
    let img_size = (total_blocks as usize * ONYFS_BLOCK_SIZE + 511) & !511;
    let mut img = vec![0u8; img_size];

    let inode_table_start = 3u32;
    let snapshot_start = if !v1 { 3 + inode_table_blocks } else { 0 };
    let journal_start = if !v1 {
        snapshot_start + snapshot_blocks
    } else {
        0
    };
    let data_blocks_start = metadata_blocks;

    if v1 {
        superblock::write_v1(
            &mut img,
            total_blocks,
            total_inodes,
            inode_table_start,
            data_blocks_start,
        );
    } else {
        superblock::write_v2(
            &mut img,
            total_blocks,
            total_inodes,
            inode_table_start,
            data_blocks_start,
            snapshot_start,
            journal_start,
            journal_blocks,
        );
    }
    inode::write_bitmaps(&mut img, total_inodes, data_blocks_needed);
    inode::write_table(
        &mut img,
        &dirs,
        &files,
        data_blocks_start,
        inode_table_start,
        v1,
    );
    dirent::write_blocks(&mut img, &dirs, &files, data_blocks_start, v1);

    File::create(&output_path)
        .unwrap_or_else(|e| {
            eprintln!("create {}: {}", output_path, e);
            process::exit(1);
        })
        .write_all(&img)
        .unwrap();
    eprintln!(
        "mkimage: v{} {} -> {} ({} blocks, {} bytes, {} inodes)",
        if v1 { 1 } else { 2 },
        &args[1],
        output_path,
        total_blocks,
        img.len(),
        total_inodes
    );
}
