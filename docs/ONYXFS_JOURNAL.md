# OnyxFS metadata crash recovery

The writer uses a bounded full-block **undo** log. This replaces the old
write-through redo log whose rollover could commit freed bitmap bits while
an inode still referenced those blocks. The data format and inode layout
are unchanged; journal records have a new, explicitly tagged format.

## Record format and ordering

The first journal block contains `ONJU` (little-endian u32), version 1,
count at byte 8, then up to 31 `(target_block, image_checksum)` pairs at byte
16. Its first 508 bytes have an FNV-1a checksum stored at byte 508. Remaining
journal blocks hold complete 4096-byte before-images, including the last eight
bytes which the old format did not protect. Count must fit the reserved area.

Before a metadata home write:

1. Read the original home block into an independent buffer.
2. Save its full before-image to the next unused log slot.
3. Publish its target/checksum in the header.
4. Write the new home block.

Repeated writes to one target retain its first image and consume no extra
slots. Capacity exhaustion fails and rolls back; it never commits an arbitrary
prefix of an operation. After all home writes complete, commit replaces the
header with a valid `ONJU` header with count zero. Old image slots are ignored.

Recovery validates the whole header, all targets and every image checksum
before changing any home block. It restores all before-images and clears the
header last. An interrupted restore is idempotent. A malformed record or an
I/O failure fails closed; normal access is quarantined until recovery.

## Operation boundaries

Public mutation entry points share a transaction with their caller, so direct
boot/internal callers also roll back or quarantine on errors. VFS holds `FS_LOCK` with interrupts disabled for the whole operation. Internal
commit calls are deferred until its outer success; an error rolls back pending
metadata. Quarantine is saved with each mount context, so a failed secondary
volume does not poison root. Snapshot syscalls use a separate locked entry
point: the snapshot engine owns its own full-block restore/quarantine protocol.

Bitmaps, inodes, directory entries and indirect pointers are journaled. Normal
file data is written before the metadata commit and is **not** transactionally
restored. Operations must not reuse blocks freed earlier in that same operation
for unjournaled data; current VFS mutation paths do not do that. Independent
syscalls form independent transactions.

## Compatibility and validation limits

Mount still replays committed legacy 4088-byte redo records. It cannot repair
inconsistent home writes made by an interrupted legacy writer without a commit.
Do not downgrade a volume with a pending ONJU transaction to an older kernel.

Host tests inject failure after every completed block write for large truncate,
indirect growth and cross-directory rename, then recover and check data/ownership.
They also cover log capacity, full-image checksums, interrupted recovery, failed
commit followed by shorter transactions, and mount/snapshot isolation.

This guarantee assumes completed block writes are ordered and atomic. The
current block API has no flush/barrier primitive. Physical device write-cache
reordering and torn sector writes are not covered; checksum detection is not
proof of power-loss durability on physical storage.
