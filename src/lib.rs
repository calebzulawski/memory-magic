#![cfg(any(unix, windows))]
#![cfg_attr(not(feature = "std"), no_std)]

mod error;
pub use error::Error;

pub mod raw;
