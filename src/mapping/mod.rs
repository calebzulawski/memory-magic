#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
mod map_impl;

use once_cell::sync::OnceCell;
use std::convert::TryInto;
use std::io::Error;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FilePermissions {
    pub write: bool,
    pub execute: bool,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ViewPermissions {
    Read,
    Write,
    CopyOnWrite,
    Execute,
}

/// Shared memory
#[derive(Debug)]
pub struct MappedObject {
    inner: map_impl::MappedObject,
    size: u64,
}

impl MappedObject {
    /// Create an anonymous shared memory of `size` bytes.
    ///
    /// This memory region is readable and writable.
    pub fn anonymous(size: usize) -> Result<Self, Error> {
        Ok(Self {
            inner: map_impl::MappedObject::anonymous(size, false)?,
            size: size.try_into().unwrap(),
        })
    }

    /// Create an anonymous shared memory of `size` bytes.
    ///
    /// This memory region is readable, writable, and executable.
    pub fn anonymous_exec(size: usize) -> Result<Self, Error> {
        Ok(Self {
            inner: map_impl::MappedObject::anonymous(size, true)?,
            size: size.try_into().unwrap(),
        })
    }

    /// Map an existing file to memory.
    pub unsafe fn with_file(
        file: &std::fs::File,
        permissions: FilePermissions,
    ) -> Result<Self, Error> {
        let size = file.metadata()?.len();
        Ok(Self {
            inner: map_impl::MappedObject::with_file(file, size, permissions)?,
            size,
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Offset(u64);

impl Offset {
    pub fn granularity() -> u64 {
        static GRANULARITY: OnceCell<u64> = OnceCell::new();
        *GRANULARITY.get_or_init(|| map_impl::offset_granularity())
    }

    pub fn exact(value: u64) -> Option<Self> {
        assert!(value != 0, "length must not be zero");
        if value % Self::granularity() == 0 {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn round_up(value: u64) -> Self {
        assert!(value != 0, "length must not be zero");
        Self::exact(
            value.checked_add(Self::granularity() - 1).unwrap() / Self::granularity()
                * Self::granularity(),
        )
        .unwrap()
    }

    pub fn round_down(value: u64) -> Self {
        assert!(value != 0, "length must not be zero");
        Self::exact(value / Self::granularity() * Self::granularity()).unwrap()
    }

    pub fn to_u64(self) -> u64 {
        self.0
    }
}

impl std::convert::From<Offset> for u64 {
    fn from(value: Offset) -> Self {
        value.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Length(usize);

impl Length {
    pub fn granularity() -> usize {
        static GRANULARITY: OnceCell<usize> = OnceCell::new();
        *GRANULARITY.get_or_init(|| map_impl::length_granularity())
    }

    pub fn exact(value: usize) -> Option<Self> {
        assert!(value != 0, "length must not be zero");
        if value % Self::granularity() == 0 {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn round_up(value: usize) -> Self {
        assert!(value != 0, "length must not be zero");
        Self::exact(
            value.checked_add(Self::granularity() - 1).unwrap() / Self::granularity()
                * Self::granularity(),
        )
        .unwrap()
    }

    pub fn round_down(value: usize) -> Self {
        assert!(value != 0, "length must not be zero");
        Self::exact(value / Self::granularity() * Self::granularity()).unwrap()
    }

    pub fn to_usize(self) -> usize {
        self.0
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ViewOptions {
    offset: u64,
    length: usize,
    permissions: ViewPermissions,
}

#[derive(Copy, Clone, Debug)]
pub struct View<'a> {
    options: ViewOptions,
    mapping: &'a map_impl::MappedObject,
}

impl View<'_> {
    pub fn is_mutable(&self) -> bool {
        std::matches!(
            self.options.permissions,
            ViewPermissions::Write | ViewPermissions::CopyOnWrite
        )
    }
}

pub mod alloc {
    use super::*;

    pub fn map_length<'a>(views: &[View<'a>]) -> usize {
        views.iter().fold(0usize, |len, view| {
            len.checked_add(view.options.length).unwrap()
        })
    }

    pub fn allocate<'a>(views: &[View<'a>]) -> Result<*const u8, Error> {
        map_impl::allocate(views, false).map(|x| x as *const u8)
    }

    pub fn allocate_mut<'a>(views: &[View<'a>]) -> Result<*mut u8, Error> {
        map_impl::allocate(views, true)
    }

    pub unsafe fn deallocate(
        map: *const u8,
        view_lengths: impl Iterator<Item = usize>,
    ) -> Result<(), Error> {
        map_impl::deallocate(map as *mut u8, view_lengths)
    }

    pub unsafe fn deallocate_mut(
        map: *mut u8,
        view_lengths: impl Iterator<Item = usize>,
    ) -> Result<(), Error> {
        map_impl::deallocate(map, view_lengths)
    }
}
