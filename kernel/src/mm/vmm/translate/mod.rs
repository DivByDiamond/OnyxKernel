#[cfg(target_pointer_width = "64")]
mod sv39;
#[cfg(target_pointer_width = "32")]
mod sv32;

#[cfg(target_pointer_width = "64")]
pub use sv39::*;
#[cfg(target_pointer_width = "32")]
pub use sv32::*;
