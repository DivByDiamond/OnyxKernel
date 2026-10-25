//! Packed snapshot storage. Legacy header anchors remain reserved so older
//! kernels see a magic greater than 31 and refuse new snapshots before writes.
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, OnyfsSuper, SnapshotMeta};

pub(super) type Block = [u8; ONYFS_BLOCK_SIZE];
pub(super) const PACKED: u32 = 1;
pub(super) const PENDING: u32 = 2;
pub(super) const MAGIC: u32 = 0x32504e53;
pub(super) const STRIDE: u32 = 64;
pub(super) const RAW: u32 = 1 << 31;
pub(super) const DEFLATE: u32 = 1 << 30;
pub(super) const LENGTH_MASK: u32 = DEFLATE - 1;
pub(super) trait Io {
    fn read(&mut self, block: u32, data: &mut Block) -> KResult<()>;
    fn write(&mut self, block: u32, data: &Block) -> KResult<()>;
}
#[derive(Clone, Copy)]
pub(super) struct Area {
    pub start: u32,
    pub end: u32,
    pub slots: u32,
}
impl Area {
    pub fn new(sb: &OnyfsSuper) -> KResult<Self> {
        let start = sb.snapshot_area_start;
        let end = if sb.journal_start != 0 {
            sb.journal_start
        } else {
            sb.data_blocks_start
        };
        let table_bytes = sb.inode_count.checked_mul(128).ok_or(Errno::Io)?;
        let table_blocks = table_bytes.checked_add(4095).ok_or(Errno::Io)? / 4096;
        let table_end = sb
            .inode_table_start
            .checked_add(table_blocks)
            .ok_or(Errno::Io)?;
        if sb.data_bitmap_start != 2
            || sb.inode_table_start < 3
            || table_blocks == 0
            || table_end > start
            || start == 0
            || start >= end
            || end > sb.data_blocks_start
            || end > sb.total_blocks
        {
            return Err(Errno::Io);
        }
        let length = end - start;
        let slots = length.saturating_sub(2) / STRIDE + 1;
        if length < 3
            || length > u32::MAX / ONYFS_BLOCK_SIZE as u32
            || sb.snapshot_count > slots.min(64)
        {
            return Err(Errno::Io);
        }
        Ok(Self {
            start,
            end,
            slots: slots.min(64),
        })
    }
    pub fn anchor(self, id: u32) -> KResult<u32> {
        if id == 0 || id > self.slots {
            return Err(Errno::NoSpace);
        }
        Ok(self.start + 1 + (id - 1) * STRIDE)
    }
    pub fn payload(self, mut at: u32) -> KResult<u32> {
        if at >= self.end {
            return Err(Errno::NoSpace);
        }
        if at <= self.start {
            at = self.start + 1;
        }
        if (at - self.start - 1).is_multiple_of(STRIDE) {
            at = at.checked_add(1).ok_or(Errno::NoSpace)?;
        }
        if at >= self.end {
            return Err(Errno::NoSpace);
        }
        Ok(at)
    }
    pub fn next(self, at: u32) -> KResult<u32> {
        self.payload(at.checked_add(1).ok_or(Errno::Io)?)
    }
}
pub(super) fn get(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
pub(super) fn put(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn checksum(mut hash: u32, bytes: &[u8]) -> u32 {
    for b in bytes {
        hash = (hash ^ u32::from(*b)).wrapping_mul(16777619);
    }
    hash
}
pub(super) fn meta<I: Io>(io: &mut I, area: Area, id: u32) -> KResult<SnapshotMeta> {
    area.anchor(id)?;
    let mut block = [0; ONYFS_BLOCK_SIZE];
    io.read(area.start, &mut block)?;
    let off = (id as usize - 1) * SnapshotMeta::SIZE;
    let entry = SnapshotMeta::from_bytes(&block[off..off + SnapshotMeta::SIZE]).ok_or(Errno::Io)?;
    if entry.id != id || entry.flags & !(PACKED | PENDING) != 0 {
        return Err(Errno::Io);
    }
    Ok(entry)
}
pub(super) fn store_meta<I: Io>(io: &mut I, area: Area, entry: &SnapshotMeta) -> KResult<()> {
    area.anchor(entry.id)?;
    let mut block = [0; ONYFS_BLOCK_SIZE];
    io.read(area.start, &mut block)?;
    let off = (entry.id as usize - 1) * SnapshotMeta::SIZE;
    block[off..off + SnapshotMeta::SIZE].copy_from_slice(&entry.to_bytes());
    io.write(area.start, &block)
}

pub(super) struct Stream<'a, I> {
    pub io: &'a mut I,
    pub area: Area,
    pub at: u32,
    buffer: Block,
    offset: usize,
    loaded: bool,
    pub bytes: u32,
    pub hash: u32,
}
impl<'a, I: Io> Stream<'a, I> {
    pub fn new(io: &'a mut I, area: Area, at: u32) -> Self {
        Self {
            io,
            area,
            at,
            buffer: [0; ONYFS_BLOCK_SIZE],
            offset: 0,
            loaded: false,
            bytes: 0,
            hash: 2166136261,
        }
    }
    pub fn write(&mut self, mut input: &[u8]) -> KResult<()> {
        while !input.is_empty() {
            if self.offset == ONYFS_BLOCK_SIZE {
                self.io.write(self.at, &self.buffer)?;
                self.at = self.area.next(self.at)?;
                self.offset = 0;
                self.buffer.fill(0);
            }
            let n = input.len().min(ONYFS_BLOCK_SIZE - self.offset);
            self.buffer[self.offset..self.offset + n].copy_from_slice(&input[..n]);
            self.hash = checksum(self.hash, &input[..n]);
            self.bytes += n as u32;
            self.offset += n;
            input = &input[n..];
        }
        Ok(())
    }
    pub fn finish(&mut self) -> KResult<()> {
        self.io.write(self.at, &self.buffer)
    }
    pub fn read(&mut self, mut output: &mut [u8]) -> KResult<()> {
        while !output.is_empty() {
            if self.offset == ONYFS_BLOCK_SIZE {
                self.at = self.area.next(self.at).map_err(|_| Errno::Io)?;
                self.offset = 0;
                self.loaded = false;
            }
            if !self.loaded {
                self.io.read(self.at, &mut self.buffer)?;
                self.loaded = true;
            }
            let n = output.len().min(ONYFS_BLOCK_SIZE - self.offset);
            output[..n].copy_from_slice(&self.buffer[self.offset..self.offset + n]);
            self.hash = checksum(self.hash, &output[..n]);
            self.bytes += n as u32;
            self.offset += n;
            output = &mut output[n..];
        }
        Ok(())
    }
}
