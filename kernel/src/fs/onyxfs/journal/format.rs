//! Full-block undo header. All control fields fit in its first 512 bytes.
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, OnyfsSuper};

pub(super) const MAGIC: u32 = u32::from_le_bytes(*b"ONJU");
pub(super) const MAX_ENTRIES: usize = 31;
const CHECKSUM_AT: usize = 508;

pub(super) fn get(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}
pub(super) fn put(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn checksum(bytes: &[u8]) -> u32 {
    super::super::checksum(2166136261, bytes)
}
pub(super) fn seal(header: &mut [u8; ONYFS_BLOCK_SIZE]) {
    put(header, 0, MAGIC);
    put(header, 4, 1);
    let sum = checksum(&header[..CHECKSUM_AT]);
    put(header, CHECKSUM_AT, sum);
}
pub(super) fn validate(header: &[u8; ONYFS_BLOCK_SIZE], sb: &OnyfsSuper) -> KResult<usize> {
    let count = get(header, 8) as usize;
    if get(header, 0) != MAGIC
        || get(header, 4) != 1
        || get(header, CHECKSUM_AT) != checksum(&header[..CHECKSUM_AT])
        || count > MAX_ENTRIES
        || count >= sb.journal_size as usize
    {
        return Err(Errno::Io);
    }
    for i in 0..count {
        let block = get(header, 16 + i * 8);
        valid_target(block, sb)?;
        if (0..i).any(|j| get(header, 16 + j * 8) == block) {
            return Err(Errno::Io);
        }
    }
    Ok(count)
}
pub(super) fn valid_target(block: u32, sb: &OnyfsSuper) -> KResult<()> {
    let end = sb
        .journal_start
        .checked_add(sb.journal_size)
        .ok_or(Errno::Io)?;
    if end > sb.total_blocks || block >= sb.total_blocks || (sb.journal_start..end).contains(&block)
    {
        return Err(Errno::Io);
    }
    Ok(())
}
