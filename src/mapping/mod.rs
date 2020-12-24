#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
mod map_impl;

use crate::error::Error;
use std::convert::TryInto;

pub enum PageAccess {
    Read,
    Write,
    CopyOnWrite,
}

pub struct MapOptions {
    access: PageAccess,
    executable: bool,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FileAccess {
    Read,
    Write,
}

/// Shared memory
pub struct Mapping {
    inner: map_impl::Mapping,
    size: u64,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FileOptions {
    access: FileAccess,
    executable: bool,
}

impl Mapping {
    /// Create an anonymous shared memory of `size` bytes.
    ///
    /// This memory region is readable and writable.
    pub fn anonymous(size: usize) -> Result<Self, Error> {
        Ok(Self {
            inner: map_impl::Mapping::anonymous(size, false)?,
            size: size.try_into().unwrap(),
        })
    }

    /// Create an anonymous shared memory of `size` bytes.
    ///
    /// This memory region is readable, writable, and executable.
    pub fn anonymous_exec(size: usize) -> Result<Self, Error> {
        Ok(Self {
            inner: map_impl::Mapping::anonymous(size, true)?,
            size: size.try_into().unwrap(),
        })
    }

    /// Map an existing file to memory.
    pub unsafe fn with_file(file: &std::fs::File, options: &FileOptions) -> Result<Self, Error> {
        let size = file.metadata()?.len();
        Ok(Self {
            inner: map_impl::Mapping::with_file(file, size, options)?,
            size,
        })
    }

    unsafe fn map(&self, offset: u64, size: usize, options: &MapOptions) -> Result<*mut u8, Error> {
        self.inner.map(offset, size, options)
    }

    unsafe fn map_hint(
        &self,
        ptr: *mut u8,
        offset: u64,
        size: usize,
        options: &MapOptions,
    ) -> Result<(), Error> {
        self.inner.map_hint(ptr, offset, size, options)
    }
}
