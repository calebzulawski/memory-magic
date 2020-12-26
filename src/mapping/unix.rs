use super::*;
use crate::error::{access_denied, to_io_error};
use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    libc::c_int,
    sys::mman::{mmap, munmap, MapFlags, ProtFlags},
    unistd::{close, ftruncate, sysconf, SysconfVar},
};
use std::convert::TryInto;
use std::io::Error;

fn open_anonymous(size: i64) -> Result<c_int, Error> {
    let fd = shm_open_anonymous::shm_open_anonymous();
    if fd == -1 {
        Err(Error::last_os_error())
    } else {
        ftruncate(fd, size).map_err(|e| {
            let _ = close(fd);
            to_io_error(e)
        })?;
        Ok(fd)
    }
}

impl ViewPermissions {
    fn prot_flags(&self) -> ProtFlags {
        match self {
            Self::Read => ProtFlags::PROT_READ,
            Self::Write | Self::CopyOnWrite => ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            Self::Execute => ProtFlags::PROT_READ | ProtFlags::PROT_EXEC,
        }
    }

    fn map_flags(&self) -> MapFlags {
        match self {
            Self::CopyOnWrite => MapFlags::MAP_PRIVATE,
            _ => MapFlags::MAP_SHARED,
        }
    }
}

#[derive(Debug)]
pub struct MappedObject {
    fd: c_int,
    executable: bool,
}

impl Drop for MappedObject {
    fn drop(&mut self) {
        let _ = close(self.fd);
    }
}

impl MappedObject {
    pub fn anonymous(size: usize, executable: bool) -> Result<Self, Error> {
        Ok(MappedObject {
            fd: open_anonymous(size.try_into().unwrap())?,
            executable,
        })
    }

    pub unsafe fn with_file(
        file: &std::fs::File,
        _size: u64,
        permissions: FilePermissions,
    ) -> Result<Self, Error> {
        let file = file.try_clone()?;
        let mapped = MappedObject {
            fd: std::os::unix::io::IntoRawFd::into_raw_fd(file),
            executable: permissions == FilePermissions::Execute,
        };

        // Check permissions for the "write" permission:
        // * The file must be opened read-write
        // * We cannot write to a file opened in append-mode with mmap
        let oflags =
            OFlag::from_bits(fcntl(mapped.fd, FcntlArg::F_GETFL).map_err(to_io_error)?).unwrap();
        let permissions_match = match permissions {
            FilePermissions::Read | FilePermissions::Execute => {
                oflags.contains(OFlag::O_RDONLY) || oflags.contains(OFlag::O_RDWR)
            }
            FilePermissions::Write => {
                oflags.contains(OFlag::O_RDWR) && !oflags.contains(OFlag::O_APPEND)
            }
        };
        if permissions_match {
            Ok(mapped)
        } else {
            Err(access_denied())
        }
    }

    unsafe fn map_impl(&self, ptr: *mut u8, view: &ViewOptions) -> Result<*mut u8, Error> {
        mmap(
            ptr as *mut _,
            view.length,
            view.permissions.prot_flags(),
            view.permissions.map_flags(),
            self.fd,
            view.offset.try_into().unwrap(),
        )
        .map(|x| x as *mut u8)
        .map_err(crate::error::to_io_error)
    }

    pub(crate) unsafe fn map(&self, view: &ViewOptions) -> Result<*mut u8, Error> {
        self.map_impl(std::ptr::null_mut(), view)
    }

    pub(crate) unsafe fn map_hint(&self, ptr: *mut u8, view: &ViewOptions) -> Result<(), Error> {
        self.map_impl(ptr, view).map(std::mem::drop)
    }
}

fn page_size() -> u64 {
    sysconf(SysconfVar::PAGE_SIZE)
        .unwrap()
        .unwrap()
        .try_into()
        .unwrap()
}

pub fn offset_granularity() -> u64 {
    page_size()
}

pub fn length_granularity() -> usize {
    page_size().try_into().unwrap()
}

pub fn allocate<'a>(views: &[View<'a>], mutable: bool) -> Result<*mut u8, Error> {
    if mutable && !views.iter().all(|view| view.is_mutable()) {
        return Err(access_denied());
    }

    // Allocate mapping
    let len = alloc::map_length(views);
    let ptr = {
        let fd = open_anonymous(len.try_into().unwrap())?;
        let ptr = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                ProtFlags::PROT_NONE,
                MapFlags::MAP_SHARED,
                fd,
                0,
            )
        };
        // close fd unconditionally before checking error
        close(fd).map_err(crate::error::to_io_error)?;
        ptr.map(|ptr| ptr as *mut u8)
            .map_err(crate::error::to_io_error)
    }?;

    let try_map = || {
        let mut offset = 0;
        for view in views {
            unsafe {
                view.mapping.map_hint(ptr.add(offset), &view.options)?;
            }
            offset += view.options.length;
        }
        Ok(ptr as *mut u8)
    };

    try_map().map_err(|e| unsafe {
        let _ = munmap(ptr as *mut _, len);
        e
    })
}

pub unsafe fn deallocate(
    map: *const u8,
    view_lengths: impl Iterator<Item = usize>,
) -> Result<(), Error> {
    let len = view_lengths.fold(0usize, |len, view_len| len.checked_add(view_len).unwrap());
    munmap(map as *mut _, len).map_err(to_io_error)
}
