use onyx_core::formats::{
    ONYFS_BLOCK_SIZE, ONYFS_FEAT_JOURNAL, ONYFS_FEAT_SNAPSHOTS, ONYFS_FEAT_TIMESTAMPS, ONYFS_MAGIC,
    ONYFS_MAGIC_V1, ONYFS_VERSION,
};

const V2_SUPERBLOCK_SIZE: usize = 128;

pub fn write_v1(
    img: &mut [u8],
    total_blocks: u32,
    inode_count: u32,
    inode_table_start: u32,
    data_blocks_start: u32,
) {
    let sb = [
        ONYFS_MAGIC_V1.to_le_bytes(),
        1u32.to_le_bytes(),
        (ONYFS_BLOCK_SIZE as u32).to_le_bytes(),
        total_blocks.to_le_bytes(),
        inode_count.to_le_bytes(),
        inode_table_start.to_le_bytes(),
        2u32.to_le_bytes(),
        data_blocks_start.to_le_bytes(),
        1u32.to_le_bytes(),
    ];
    let mut off = 0;
    for chunk in &sb {
        img[off..off + 4].copy_from_slice(chunk);
        off += 4;
    }
}

/// Layout values for the v2 superblock, grouped so `write_v2` takes a
/// single parameter instead of seven scalars.
pub struct V2Layout {
    pub total_blocks: u32,
    pub inode_count: u32,
    pub inode_table_start: u32,
    pub data_blocks_start: u32,
    pub snapshot_area_start: u32,
    pub journal_start: u32,
    pub journal_size: u32,
}

pub fn write_v2(img: &mut [u8], layout: &V2Layout) {
    let V2Layout {
        total_blocks,
        inode_count,
        inode_table_start,
        data_blocks_start,
        snapshot_area_start,
        journal_start,
        journal_size,
    } = *layout;
    let feature_flags: u32 = ONYFS_FEAT_TIMESTAMPS | ONYFS_FEAT_SNAPSHOTS | ONYFS_FEAT_JOURNAL;
    let mut sb = [0u8; V2_SUPERBLOCK_SIZE];
    sb[0..4].copy_from_slice(&ONYFS_MAGIC.to_le_bytes());
    sb[4..8].copy_from_slice(&ONYFS_VERSION.to_le_bytes());
    sb[8..12].copy_from_slice(&(ONYFS_BLOCK_SIZE as u32).to_le_bytes());
    sb[12..16].copy_from_slice(&total_blocks.to_le_bytes());
    sb[16..20].copy_from_slice(&inode_count.to_le_bytes());
    sb[20..24].copy_from_slice(&inode_table_start.to_le_bytes());
    sb[24..28].copy_from_slice(&2u32.to_le_bytes());
    sb[28..32].copy_from_slice(&data_blocks_start.to_le_bytes());
    sb[32..36].copy_from_slice(&1u32.to_le_bytes());
    sb[36..40].copy_from_slice(&snapshot_area_start.to_le_bytes());
    sb[40..44].copy_from_slice(&0u32.to_le_bytes());
    sb[44..48].copy_from_slice(&journal_start.to_le_bytes());
    sb[48..52].copy_from_slice(&journal_size.to_le_bytes());
    sb[52..56].copy_from_slice(&feature_flags.to_le_bytes());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    sb[56..64].copy_from_slice(&ts.to_le_bytes());
    img[0..V2_SUPERBLOCK_SIZE].copy_from_slice(&sb);
}
