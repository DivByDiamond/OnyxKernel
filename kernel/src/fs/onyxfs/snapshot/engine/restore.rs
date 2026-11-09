//! Restore paths: packed archives, legacy readers and restartable rollback.
use super::compress::rle_decompress;
use super::super::format::{
    Area, Block, DEFLATE, Io, LENGTH_MASK, PACKED, PENDING, RAW, get, meta, store_meta,
};
use super::super::stream::Stream;
use super::{header, payload_end, persist, target};
use miniz_oxide::inflate::{
    TINFLStatus,
    core::{DecompressorOxide, decompress, inflate_flags},
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, OnyfsSuper, SnapshotMeta};

fn decode(input: &[u8], output: &mut Block) -> KResult<()> {
    // The decoder rejects overflow, truncation and trailing packets; every
    // successful archive record must expand to exactly one complete block.
    if unsafe { rle_decompress(input, output) } == ONYFS_BLOCK_SIZE {
        Ok(())
    } else {
        Err(Errno::Io)
    }
}
fn packed<I: Io>(
    io: &mut I,
    sb: &OnyfsSuper,
    area: Area,
    m: SnapshotMeta,
    apply: bool,
) -> KResult<()> {
    let h = header(io, area, m.id)?;
    payload_end(area, h)?;
    if h.count != m.block_count {
        return Err(Errno::Io);
    }
    let mut stream = Stream::new(io, area, h.start);
    let mut prev = 0;
    let mut data = [0; ONYFS_BLOCK_SIZE];
    let mut compressed = [0; 8192];
    for _ in 0..h.count {
        if h.bytes.saturating_sub(stream.bytes) < 8 {
            return Err(Errno::Io);
        }
        let mut entry = [0; 8];
        stream.read(&mut entry)?;
        let at = get(&entry, 0);
        let encoded = get(&entry, 4);
        let length = (encoded & LENGTH_MASK) as usize;
        target(sb, at)?;
        if at <= prev
            || length == 0
            || length > ONYFS_BLOCK_SIZE
            || length as u32 > h.bytes.saturating_sub(stream.bytes)
        {
            return Err(Errno::Io);
        }
        prev = at;
        if encoded & RAW != 0 {
            if encoded & DEFLATE != 0 || length != ONYFS_BLOCK_SIZE {
                return Err(Errno::Io);
            }
            stream.read(&mut data)?;
        } else {
            stream.read(&mut compressed[..length])?;
            if encoded & DEFLATE != 0 {
                let mut decompressor = DecompressorOxide::new();
                let (status, consumed, written) = decompress(
                    &mut decompressor,
                    &compressed[..length],
                    &mut data,
                    0,
                    inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
                );
                if status != TINFLStatus::Done || consumed != length || written != ONYFS_BLOCK_SIZE
                {
                    return Err(Errno::Io);
                }
            } else {
                decode(&compressed[..length], &mut data)?;
            }
        }
        if apply {
            stream.io.write(at, &data)?;
        }
    }
    if stream.bytes != h.bytes || stream.hash != h.hash {
        return Err(Errno::Io);
    }
    Ok(())
}
fn legacy<I: Io>(
    io: &mut I,
    sb: &OnyfsSuper,
    area: Area,
    m: SnapshotMeta,
    apply: bool,
) -> KResult<()> {
    let anchor = area.anchor(m.id)?;
    let mut hdr = [0; ONYFS_BLOCK_SIZE];
    io.read(anchor, &mut hdr)?;
    let n = get(&hdr, 0);
    if n > 31 || n != m.block_count || anchor.checked_add(1 + 2 * n).ok_or(Errno::Io)? > area.end {
        return Err(Errno::Io);
    }
    let mut data = [0; ONYFS_BLOCK_SIZE];
    let mut compressed = [0; 8192];
    for i in 0..n as usize {
        let at = get(&hdr, 4 + 8 * i);
        let len = get(&hdr, 8 + 8 * i) as usize;
        target(sb, at)?;
        if len == 0 || len > 8192 {
            return Err(Errno::Io);
        }
        let pos = anchor + 1 + 2 * i as u32;
        io.read(pos, &mut data)?;
        compressed[..ONYFS_BLOCK_SIZE].copy_from_slice(&data);
        io.read(pos + 1, &mut data)?;
        compressed[ONYFS_BLOCK_SIZE..].copy_from_slice(&data);
        if len == ONYFS_BLOCK_SIZE {
            data.copy_from_slice(&compressed[..ONYFS_BLOCK_SIZE]);
        } else {
            decode(&compressed[..len], &mut data)?;
        }
        if apply {
            io.write(at, &data)?;
        }
    }
    Ok(())
}
fn walk<I: Io>(
    io: &mut I,
    sb: &OnyfsSuper,
    area: Area,
    m: SnapshotMeta,
    apply: bool,
) -> KResult<()> {
    if m.flags & PACKED != 0 {
        packed(io, sb, area, m, apply)
    } else {
        legacy(io, sb, area, m, apply)
    }
}
pub(in crate::fs::onyxfs::snapshot) fn rollback<I: Io>(
    io: &mut I,
    sb: &mut OnyfsSuper,
    id: u32,
    quarantined: &mut bool,
) -> KResult<()> {
    let area = Area::new(sb)?;
    if id == 0 || id > sb.snapshot_count {
        return Err(Errno::NoEnt);
    }
    let mut m = meta(io, area, id)?;
    if m.root_inode_snapshot == 0 || m.root_inode_snapshot > sb.inode_count {
        return Err(Errno::Io);
    }
    // Validate all targets, lengths and checksum before changing any live block.
    walk(io, sb, area, m, false)?;
    *quarantined = true;
    m.flags |= PENDING;
    store_meta(io, area, &m)?;
    // The durable archive itself is a full-block redo source. A crash repeats
    // this idempotent restore on mount, without the old journal's 4088-byte tail
    // truncation or finite transaction-entry limit.
    walk(io, sb, area, m, true)?;
    sb.root_inode = m.root_inode_snapshot;
    persist(io, sb)?;
    m.flags &= !PENDING;
    store_meta(io, area, &m)?;
    *quarantined = false;
    Ok(())
}
pub(in crate::fs::onyxfs::snapshot) fn recover<I: Io>(
    io: &mut I,
    sb: &mut OnyfsSuper,
    quarantined: &mut bool,
) -> KResult<()> {
    if sb.snapshot_area_start == 0 {
        return Ok(());
    }
    let area = Area::new(sb)?;
    let mut pending = 0;
    for id in 1..=sb.snapshot_count {
        if meta(io, area, id)?.flags & PENDING != 0 {
            if pending != 0 {
                return Err(Errno::Io);
            }
            pending = id;
        }
    }
    if pending != 0 {
        rollback(io, sb, pending, quarantined)?;
    }
    *quarantined = false;
    Ok(())
}
