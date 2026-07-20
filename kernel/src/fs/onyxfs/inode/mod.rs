pub mod read;
pub mod write;

pub(super) use read::read_inode;
pub use read::stat;
pub use write::set_mode;
pub use write::set_timestamps;
pub use write::update_mtime;
pub(super) use write::write_inode;
