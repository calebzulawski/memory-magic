#![cfg(any(unix, windows))]

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
pub(crate) mod map_impl;

pub mod view;
pub mod raw;
