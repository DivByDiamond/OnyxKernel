//! FAT32 write support — extends the read-only driver with file write,
//! create, unlink, and chain extension.
//!
//! All operations go through the FAT to allocate/free clusters and chain
//! them into existing files. LFN (long file name) is NOT supported —
//! all new files are created with 8.3 short names (case-insensitive
//! lookup is handled by fat32_name_8_3 in helpers.rs).
//!
//! Concurrency: there is no per-FS lock; the kernel is single-threaded
//! with respect to virtio_blk requests at the time of writing. When SMP
//! is fully wired for FS I/O, callers must hold a global FAT32 lock.

mod data;
mod dirent;
mod fat;
mod ops;

pub use data::{truncate_chain, write};
pub use ops::{create, unlink, update_size};
