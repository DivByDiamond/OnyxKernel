//! Snapshot creation: live-block enumeration, allocation, compression and
//! metadata persist.
use super::format::*;
use super::stream::Stream;
use alloc::{boxed::Box, vec::Vec};
use miniz_oxide::deflate::core::{
    CompressorOxide, TDEFLFlush, TDEFLStatus, compress, create_comp_flags_from_zip_params,
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    ONYFS_BLOCK_SIZE, ONYFS_FEAT_SNAPSHOTS, OnyfsInode, OnyfsSuper, SnapshotMeta,
};

fn table_blocks(sb: &OnyfsSuper) -> KResult<u32> {
    let bytes = sb
        .inode_count
        .checked_mul(OnyfsInode::SIZE as u32)
        .ok_or(Errno::Io)?;
    bytes
        .checked_add(ONYFS_BLOCK_SIZE as u32 - 1)
        .map(|n| n / ONYFS_BLOCK_SIZE as u32)
        .ok_or(Errno::Io)
}
fn target(sb: &OnyfsSuper, at: u32) -> KResult<()> {
    if at == 1
        || at == sb.data_bitmap_start
        || (at >= sb.inode_table_start
            && at
                < sb.inode_table_start
                    .checked_add(table_blocks(sb)?)
                    .ok_or(Errno::Io)?)
        || (at >= sb.data_blocks_start && at < sb.total_blocks)
    {
        Ok(())
    } else {
        Err(Errno::Io)
    }
}
fn live_blocks<I: Io>(io: &mut I, sb: &OnyfsSuper) -> KResult<Vec<u32>> {
    let mut bitmap = [0; ONYFS_BLOCK_SIZE];
    io.read(sb.data_bitmap_start, &mut bitmap)?;
    let data_count = sb
        .total_blocks
        .checked_sub(sb.data_blocks_start)
        .ok_or(Errno::Io)?;
    if data_count > (ONYFS_BLOCK_SIZE * 8) as u32 {
        return Err(Errno::Io);
    }
    let mut blocks = Vec::new();
    let count = bitmap
        .iter()
        .map(|x| x.count_ones() as usize)
        .sum::<usize>()
        + table_blocks(sb)? as usize
        + 2;
    blocks.try_reserve_exact(count).map_err(|_| Errno::NoMem)?;
    blocks.push(1);
    blocks.push(sb.data_bitmap_start);
    for n in 0..table_blocks(sb)? {
        blocks.push(sb.inode_table_start + n);
    }
    for n in 0..data_count {
        if bitmap[n as usize / 8] & (1 << (n % 8)) != 0 {
            blocks.push(sb.data_blocks_start + n);
        }
    }
    if blocks.windows(2).any(|w| w[0] >= w[1]) {
        return Err(Errno::Io);
    }
    for &at in &blocks {
        target(sb, at)?;
    }
    Ok(blocks)
}
#[derive(Clone, Copy)]
struct Header {
    count: u32,
    bytes: u32,
    hash: u32,
    start: u32,
}
fn header<I: Io>(io: &mut I, area: Area, id: u32) -> KResult<Header> {
    let mut b = [0; ONYFS_BLOCK_SIZE];
    io.read(area.anchor(id)?, &mut b)?;
    if get(&b, 0) != MAGIC {
        return Err(Errno::Io);
    }
    let h = Header {
        count: get(&b, 4),
        bytes: get(&b, 8),
        hash: get(&b, 12),
        start: get(&b, 16),
    };
    if h.count == 0
        || h.count > h.bytes / 9
        || h.bytes > (area.end - area.start) * ONYFS_BLOCK_SIZE as u32
        || area.payload(h.start).map_err(|_| Errno::Io)? != h.start
    {
        return Err(Errno::Io);
    }
    Ok(h)
}
fn payload_end(area: Area, h: Header) -> KResult<u32> {
    let mut at = h.start;
    let blocks = h.bytes.div_ceil(ONYFS_BLOCK_SIZE as u32);
    for _ in 1..blocks {
        at = area.next(at).map_err(|_| Errno::Io)?;
    }
    at.checked_add(1).ok_or(Errno::Io)
}
fn allocation<I: Io>(io: &mut I, sb: &OnyfsSuper, area: Area, bytes: u32) -> KResult<u32> {
    let mut end = area.start + 1;
    for id in 1..=sb.snapshot_count {
        let m = meta(io, area, id)?;
        if m.flags & PENDING != 0 {
            return Err(Errno::Busy);
        }
        let next = if m.flags & PACKED != 0 {
            payload_end(area, header(io, area, id)?)?
        } else {
            (area.anchor(id)? + STRIDE).min(area.end)
        };
        end = end.max(next);
    }
    let start = area.payload(end)?;
    payload_end(
        area,
        Header {
            count: 1,
            bytes,
            hash: 0,
            start,
        },
    )
    .map_err(|_| Errno::NoSpace)?;
    Ok(start)
}
fn encoding(raw: &Block, compressed: &mut [u8; 8192], compressor: &mut CompressorOxide) -> u32 {
    compressor.reset();
    let (status, read, written) = compress(compressor, raw, compressed, TDEFLFlush::Finish);
    if status == TDEFLStatus::Done && read == raw.len() && written < raw.len() {
        DEFLATE | written as u32
    } else {
        RAW | ONYFS_BLOCK_SIZE as u32
    }
}
pub(super) fn create<I: Io>(
    io: &mut I,
    sb: &mut OnyfsSuper,
    name: &[u8],
    time: u64,
) -> KResult<u32> {
    if sb.feature_flags & ONYFS_FEAT_SNAPSHOTS == 0 || sb.snapshot_area_start == 0 {
        return Err(Errno::NoSys);
    }
    let area = Area::new(sb)?;
    let id = sb.snapshot_count.checked_add(1).ok_or(Errno::NoSpace)?;
    area.anchor(id)?;
    let blocks = live_blocks(io, sb)?;
    let flags = create_comp_flags_from_zip_params(6, 0, 0);
    // Heap destination prevents the compressor's 64-KiB code buffer from
    // consuming the process's 64-KiB kernel stack.
    let mut compressor = Box::write(Box::new_uninit(), CompressorOxide::new(flags));
    let mut raw = [0; ONYFS_BLOCK_SIZE];
    let mut encoded = [0; 8192];
    let mut bytes = 0u32;
    for &at in &blocks {
        io.read(at, &mut raw)?;
        bytes = bytes
            .checked_add(8 + (encoding(&raw, &mut encoded, &mut compressor) & LENGTH_MASK))
            .ok_or(Errno::NoSpace)?;
    }
    let start = allocation(io, sb, area, bytes)?;
    let mut stream = Stream::new(io, area, start);
    for &at in &blocks {
        stream.io.read(at, &mut raw)?;
        let length = encoding(&raw, &mut encoded, &mut compressor);
        let mut entry = [0; 8];
        put(&mut entry, 0, at);
        put(&mut entry, 4, length);
        stream.write(&entry)?;
        stream.write(if length & RAW != 0 {
            &raw
        } else {
            &encoded[..(length & LENGTH_MASK) as usize]
        })?;
    }
    stream.finish()?;
    let h = Header {
        count: blocks.len() as u32,
        bytes: stream.bytes,
        hash: stream.hash,
        start,
    };
    let mut b = [0; ONYFS_BLOCK_SIZE];
    put(&mut b, 0, MAGIC);
    put(&mut b, 4, h.count);
    put(&mut b, 8, h.bytes);
    put(&mut b, 12, h.hash);
    put(&mut b, 16, h.start);
    io.write(area.anchor(id)?, &b)?;
    let mut names = [0; 32];
    let n = name.len().min(32);
    names[..n].copy_from_slice(&name[..n]);
    store_meta(
        io,
        area,
        &SnapshotMeta {
            id,
            timestamp: time,
            root_inode_snapshot: sb.root_inode,
            block_count: h.count,
            name: names,
            parent_id: 0,
            flags: PACKED,
            reserved: [0; 4],
        },
    )?;
    let old = sb.snapshot_count;
    sb.snapshot_count = id;
    if let Err(e) = persist(io, sb) {
        sb.snapshot_count = old;
        return Err(e);
    }
    Ok(id)
}
pub(super) fn persist<I: Io>(io: &mut I, sb: &OnyfsSuper) -> KResult<()> {
    let mut b = [0; ONYFS_BLOCK_SIZE];
    io.read(0, &mut b)?;
    let s = sb.to_bytes();
    b[..s.len()].copy_from_slice(&s);
    io.write(0, &b)
}

mod compress;
mod restore;

pub(super) use restore::{recover, rollback};

#[cfg(test)]
mod tests;
