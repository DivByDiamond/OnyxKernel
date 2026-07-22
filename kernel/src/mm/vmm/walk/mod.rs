#[cfg(target_pointer_width = "64")]
mod sv39;
#[cfg(target_pointer_width = "32")]
mod sv32;

#[cfg(target_pointer_width = "64")]
pub(super) use sv39::walk;
#[cfg(target_pointer_width = "32")]
pub(super) use sv32::walk;
