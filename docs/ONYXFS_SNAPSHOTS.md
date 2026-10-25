# OnyxFS packed snapshots

The old snapshot writer used two physical blocks for every compressed input
block inside a fixed 64-block slot, limiting snapshots to 31 blocks. It also
walked only direct inode pointers, omitting indirect file blocks and the inode
bitmap. The tested system image actually allocates 965 data blocks plus twelve
metadata blocks; the earlier direct-pointer count of 197 was incomplete.

The packed writer captures the inode bitmap, data bitmap, complete inode table,
and every allocated data-area block, including indirect tables and file data.
It uses bounded per-block raw DEFLATE with a raw fallback. The existing reserved
snapshot area remains the storage limit; no data/journal relocation or implicit
image migration occurs. A genuinely full area returns `NoSpace` before writes.
The existing system root fits in its original 1-MiB area after compression.

## Layout and old images

The 64-byte SnapshotMeta records and v2 superblock layout are unchanged. New
metadata has flag bit 0 set. Each snapshot keeps its header at the old physical
anchor `snapshot_area_start + 1 + (id-1)*64`. Packed payload skips all these
anchors. The header magic `SNP2` is greater than the old reader's maximum record
count, so an old kernel refuses a new snapshot rollback before touching data.
The current kernel continues reading old fixed-slot raw/RLE snapshots.

A packed header contains little-endian u32 magic, record count, byte length,
FNV-1a corruption checksum and payload start block. Each streamed record has a
u32 target block, a u32 encoding/length, then payload. Bit31 means a raw
4096-byte block; bit30 means raw DEFLATE; the low30 bits are the stored length.
Input and output are bounded to one block; readers validate exact DEFLATE input
consumption, output size, every target and the complete stream checksum before
applying anything. The checksum detects accidental corruption, not an attacker
who can rewrite the disk and recompute it.

**Downgrade writes are unsupported after creating packed snapshots.** An old
kernel does not understand the packed allocator or pending rollback marker;
although its rollback guard rejects packed headers, its snapshot writer can
reuse payload space. Retain backups and boot the new kernel to finish a pending
restore. This is read compatibility of the new kernel with old snapshots, not a
promise that old kernels can safely mutate new snapshot archives.

## Interrupted rollback and live I/O failure

Before changing live blocks the kernel validates the complete archive, then
publishes metadata flag bit1 (pending rollback). The immutable archive itself is
the full-block redo source; the old journal only saves 4088 bytes per block and
has too few entries for a system-root rollback. Mount finishes pending rollback
before allowing normal filesystem operations. The pending bit is cleared only
after all blocks and the root-inode superblock field have been restored.

If I/O fails after intent publication, a per-mount quarantine flag blocks normal
OnyxFS block reads/writes until recovery succeeds. A syscall therefore cannot
return a partially restored filesystem to ordinary live I/O. Mount contexts
retain independent quarantine flags for separate volumes.

Tests inject failure at every completed rollback write boundary and check both
remount replay and the live quarantine gate. These tests model failures between
completed block writes. They do not establish a hardware write-cache flush or
atomicity guarantee for torn sectors; the existing VirtIO backend must supply
its own ordered write-completion contract.

## Compression implementation

`miniz_oxide` 0.9.1 is pinned with no std feature. Its upstream compressor has a
64-KiB inline code buffer on non-Wasm platforms, which overflowed the 64-KiB
kernel stack in an actual RV64 test. A small vendored feature selects the same
heap-backed buffer path already used upstream for Wasm. Kernel stack sizes are
unchanged. Upstream notices, file hashes, crate hash and the exact local patch
are retained in `vendor/miniz_oxide/`.
