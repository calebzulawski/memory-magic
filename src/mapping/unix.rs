use super::{FileAccess, FileOptions, MapOptions, PageAccess};
use crate::error::Error;
use nix::{
    errno::Errno,
    fcntl::{fcntl, FcntlArg, OFlag},
    libc::c_int,
    sys::mman::{mmap, MapFlags, ProtFlags},
    unistd::{close, ftruncate, sysconf, SysconfVar},
};
use std::convert::TryInto;

impl MapOptions {
    fn prot_flags(&self) -> ProtFlags {
        let mut flags = match self.access {
            PageAccess::Read => ProtFlags::PROT_READ,
            _ => ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
        };
        if self.executable {
            flags |= ProtFlags::PROT_EXEC;
        }
        flags
    }

    fn map_flags(&self) -> MapFlags {
        match self.access {
            PageAccess::CopyOnWrite => MapFlags::MAP_PRIVATE,
            _ => MapFlags::MAP_SHARED,
        }
    }
}

pub struct Mapping {
    fd: c_int,
    executable: bool,
}

impl Drop for Mapping {
    fn drop(&mut self) {
        debug_assert!(close(self.fd).is_ok());
    }
}

impl Mapping {
    pub fn anonymous(size: usize, executable: bool) -> Result<Self, Error> {
        let mapped = Mapping {
            fd: shm_open_anonymous::shm_open_anonymous(),
            executable,
        };
        if mapped.fd == -1 {
            Err(std::io::Error::last_os_error().into())
        } else {
            ftruncate(mapped.fd, size.try_into().unwrap())?;
            Ok(mapped)
        }
    }

    pub unsafe fn with_file(
        file: &std::fs::File,
        _size: u64,
        options: &FileOptions,
    ) -> Result<Self, Error> {
        let file = file.try_clone()?;
        let mapped = Mapping {
            fd: std::os::unix::io::IntoRawFd::into_raw_fd(file),
            executable: options.executable,
        };

        // Check permissions:
        // * The file must be opened read-write
        // * We cannot write to a file opened in append-mode with mmap
        let oflags = OFlag::from_bits(fcntl(mapped.fd, FcntlArg::F_GETFL)?).unwrap();
        if options.access == FileAccess::Write
            && (!oflags.contains(OFlag::O_RDWR) || oflags.contains(OFlag::O_APPEND))
        {
            Err(nix::Error::from_errno(Errno::EACCES).into())
        } else {
            Ok(mapped)
        }
    }

    unsafe fn map_impl(
        &self,
        ptr: *mut u8,
        offset: u64,
        size: usize,
        options: &MapOptions,
    ) -> Result<*mut u8, Error> {
        mmap(
            ptr as *mut _,
            size.try_into().unwrap(),
            options.prot_flags(),
            options.map_flags(),
            self.fd,
            offset.try_into().unwrap(),
        )
        .map(|x| x as *mut u8)
        .map_err(Into::into)
    }

    pub unsafe fn map(
        &self,
        offset: u64,
        size: usize,
        options: &MapOptions,
    ) -> Result<*mut u8, Error> {
        self.map_impl(std::ptr::null_mut(), offset, size, options)
    }

    pub unsafe fn map_hint(
        &self,
        ptr: *mut u8,
        offset: u64,
        size: usize,
        options: &MapOptions,
    ) -> Result<(), Error> {
        self.map_impl(ptr, offset, size, options)
            .map(std::mem::drop)
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
