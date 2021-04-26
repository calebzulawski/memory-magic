#![cfg(any(unix, windows))]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
pub(crate) mod map_impl;

mod error;
pub use error::Error;

pub mod raw;
pub mod view;
