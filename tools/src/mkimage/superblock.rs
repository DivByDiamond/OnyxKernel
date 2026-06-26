const ONYFS_MAGIC_V2: u32 = 0x32594E4F;
const ONYFS_MAGIC_V1: u32 = 0x31594E4F;
const V2_SUPERBLOCK_SIZE: usize = 128;

pub fn write_v1(img: &mut [u8], total_blocks: u32, inode_count: u32, inode_table_start: u32, data_blocks_start: u32) {
    let sb = [
        ONYFS_MAGIC_V1.to_le_bytes(),
        1u32.to_le_bytes(),
        4096u32.to_le_bytes(),
        total_blocks.to_le_bytes(),
        inode_count.to_le_bytes(),
        inode_table_start.to_le_bytes(),
        2u32.to_le_bytes(),
        data_blocks_start.to_le_bytes(),
        1u32.to_le_bytes(),
    ];
    let mut off = 0;
    for chunk in &sb { img[off..off + 4].copy_from_slice(chunk); off += 4; }
}

pub fn write_v2(
    img: &mut [u8], total_blocks: u32, inode_count: u32,
    inode_table_start: u32, data_blocks_start: u32,
    snapshot_area_start: u32, journal_start: u32, journal_size: u32,
) {
    let feature_flags: u32 = 0x1 | 0x2 | 0x8;
    let mut sb = [0u8; V2_SUPERBLOCK_SIZE];
    sb[0..4].copy_from_slice(&ONYFS_MAGIC_V2.to_le_bytes());
    sb[4..8].copy_from_slice(&2u32.to_le_bytes());
    sb[8..12].copy_from_slice(&4096u32.to_le_bytes());
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
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    sb[56..64].copy_from_slice(&ts.to_le_bytes());
    img[0..V2_SUPERBLOCK_SIZE].copy_from_slice(&sb);
}
