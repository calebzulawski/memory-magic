//! Views of objects mapped to shared memory.

use crate::map_impl;
use crate::Error;
use core::convert::TryInto;
use once_cell::race::OnceNonZeroUsize;

/// Permissions for file mapping.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FilePermissions {
    /// The file is writable.
    pub write: bool,
    /// The file is executable.
    pub execute: bool,
}

/// Permissions for a particular view of an object.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ReadPermissions {
    /// the view is read-only
    Read,
    /// the view is readable and executable
    Execute,
}

/// Permissions for a particular mutable view of an object.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum WritePermissions {
    /// the view is readable and writable
    Write,
    /// the view is readable and writable, but writes do not propagate to the backing object
    CopyOnWrite,
}

/// An object that can be mapped to virtual memory.
#[derive(Debug)]
pub struct Object {
    inner: map_impl::Object,
    size: u64,
    write: bool,
    execute: bool,
}

impl Object {
    /// Create an anonymous shared memory of `size` bytes.
    ///
    /// This memory region is always writable.
    pub fn anonymous(size: usize, permissions: ReadPermissions) -> Result<Self, Error> {
        let execute = permissions == ReadPermissions::Execute;
        Ok(Self {
            inner: map_impl::Object::anonymous(size, execute)?,
            size: size.try_into().unwrap(),
            write: true,
            execute,
        })
    }

    /// Map an existing file to memory.
    ///
    /// # Safety
    /// See [`FileOptions::new`].
    #[cfg(feature = "std")]
    pub unsafe fn with_file(file: &std::fs::File) -> FileOptions<'_> {
        FileOptions::new(file)
    }

    /// Create a view of the mapped object.
    ///
    /// Returns `None` if the requested permissions are not allowed for this object.
    pub fn view(
        &self,
        offset: Offset,
        length: Length,
        permissions: ReadPermissions,
    ) -> Option<View<'_>> {
        let execute = permissions == ReadPermissions::Execute;
        if !execute || self.execute {
            Some(View {
                offset,
                length,
                execute,
                object: &self.inner,
            })
        } else {
            None
        }
    }

    /// Create a mutable view of the mapped object.
    ///
    /// Returns `None` if the requested permissions are not allowed for this object.
    pub fn view_mut(
        &self,
        offset: Offset,
        length: Length,
        permissions: WritePermissions,
    ) -> Option<ViewMut<'_>> {
        let copy_on_write = permissions == WritePermissions::CopyOnWrite;
        if copy_on_write || self.write {
            Some(ViewMut {
                offset,
                length,
                copy_on_write,
                object: &self.inner,
            })
        } else {
            None
        }
    }
}

/// Options for opening a file mapping.
#[cfg(feature = "std")]
pub struct FileOptions<'a> {
    file: &'a std::fs::File,
    write: bool,
    execute: bool,
}

#[cfg(feature = "std")]
impl<'a> FileOptions<'a> {
    /// Create a new set of options for opening a file mapping.
    ///
    /// All options are initially set to `false`.
    ///
    /// # Safety
    /// Using this function means you are guaranteeing that the file will never be truncated or
    /// modified external to this file mapping.
    /// In most cases it's impossible to actually guarantee a file will never be modified.
    /// For example, on Linux a pathological program can open the file the instant it is created,
    /// and truncate or modify the file after it's mapped to memory.
    /// If the file is truncated, it can result in SIGBUS.
    /// If it's modified, it can lead to undefined behavior.
    ///
    /// Using this function indicates that you are aware of the risks and can reasonably assume the
    /// file is safe to map.
    pub unsafe fn new(file: &'a std::fs::File) -> Self {
        Self {
            file,
            write: false,
            execute: false,
        }
    }

    /// Make the file mapping writable.
    pub fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }

    /// Make the file mapping executable.
    pub fn execute(&mut self, execute: bool) -> &mut Self {
        self.execute = execute;
        self
    }

    /// Open a mapping object with the specified options.
    pub fn finish(&self) -> Result<Object, Error> {
        let size = self
            .file
            .metadata()
            .map_err(|e| Error(e.raw_os_error().unwrap_or(0)))?
            .len();
        // Safety: unsafe is pushed off to `new`
        let inner =
            unsafe { map_impl::Object::with_file(self.file, size, self.write, self.execute)? };
        Ok(Object {
            inner,
            size,
            write: self.write,
            execute: self.execute,
        })
    }
}

/// An offset into an object where a view may begin.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Offset(u64);

impl Offset {
    /// Offsets must be a multiple of this value.
    pub fn granularity() -> u64 {
        static GRANULARITY: OnceNonZeroUsize = OnceNonZeroUsize::new();
        GRANULARITY.get_or_init(map_impl::offset_granularity).get() as u64
    }

    /// Create an offset with the specified value.
    ///
    /// If the value is not a multiple of [`granularity`](`Self::granularity`), returns `None`.
    pub fn exact(value: u64) -> Option<Self> {
        assert!(value != 0, "length must not be zero");
        if value % Self::granularity() == 0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Create an offset, rounded up to the next possible value.
    pub fn round_up(value: u64) -> Self {
        assert!(value != 0, "length must not be zero");
        Self::exact(
            value.checked_add(Self::granularity() - 1).unwrap() / Self::granularity()
                * Self::granularity(),
        )
        .unwrap()
    }

    /// Create an offset, rounded down to the next possible value.
    pub fn round_down(value: u64) -> Self {
        assert!(value != 0, "length must not be zero");
        Self::exact(value / Self::granularity() * Self::granularity()).unwrap()
    }

    /// Get the offset value.
    pub fn to_u64(self) -> u64 {
        self.0
    }
}

impl core::convert::From<Offset> for u64 {
    fn from(value: Offset) -> Self {
        value.0
    }
}

/// A length of a view into an object.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Length(usize);

impl Length {
    /// Lengths must be a multiple of this value.
    pub fn granularity() -> usize {
        static GRANULARITY: OnceNonZeroUsize = OnceNonZeroUsize::new();
        GRANULARITY.get_or_init(map_impl::length_granularity).get()
    }

    /// Create a length with the specified value.
    ///
    /// If the value is not a multiple of [`granularity`](`Self::granularity`), returns `None`.
    pub fn exact(value: usize) -> Option<Self> {
        assert!(value != 0, "length must not be zero");
        if value % Self::granularity() == 0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Create a length, rounded up to the next possible value.
    pub fn round_up(value: usize) -> Self {
        assert!(value != 0, "length must not be zero");
        Self::exact(
            value.checked_add(Self::granularity() - 1).unwrap() / Self::granularity()
                * Self::granularity(),
        )
        .unwrap()
    }

    /// Create a length, rounded down to the next possible value.
    pub fn round_down(value: usize) -> Self {
        assert!(value != 0, "length must not be zero");
        Self::exact(value / Self::granularity() * Self::granularity()).unwrap()
    }

    /// Get the length value.
    pub fn to_usize(self) -> usize {
        self.0
    }
}

impl core::convert::From<Length> for usize {
    fn from(value: Length) -> Self {
        value.0
    }
}

/// A view of an object.
#[derive(Copy, Clone, Debug)]
pub struct View<'a> {
    pub(crate) offset: Offset,
    pub(crate) length: Length,
    pub(crate) execute: bool,
    pub(crate) object: &'a map_impl::Object,
}

/// A mutable view of an object.
#[derive(Copy, Clone, Debug)]
pub struct ViewMut<'a> {
    pub(crate) offset: Offset,
    pub(crate) length: Length,
    pub(crate) copy_on_write: bool,
    pub(crate) object: &'a map_impl::Object,
}
