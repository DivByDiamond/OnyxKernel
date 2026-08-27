#![no_std]
#![cfg_attr(not(test), no_builtins)]
#![warn(clippy::all)]

extern crate alloc;

pub mod crypto;
pub mod errno;
pub mod fmt;
pub mod formats;
pub mod parser;
pub mod string;

pub use errno::Errno;

pub use core::{cmp, mem, ptr, slice, str};
