use crate::view::{Length, Offset, View, ViewMut};
use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    libc::c_int,
    sys::mman::{mmap, munmap, MapFlags, ProtFlags},
    unistd::{close, ftruncate, sysconf, SysconfVar},
};
use std::convert::TryInto;
use std::io::Error;

fn access_denied() -> std::io::Error {
    std::io::Error::from_raw_os_error(nix::libc::EACCES)
}

fn to_io_error(error: nix::Error) -> std::io::Error {
    if let Some(errno) = error.as_errno() {
        if errno != nix::errno::Errno::UnknownErrno {
            let value: i32 = unsafe { std::mem::transmute(errno) };
            return std::io::Error::from_raw_os_error(value);
        }
    }
    std::io::Error::new(std::io::ErrorKind::Other, error)
}

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

trait ViewImpl {
    fn offset(&self) -> Offset;
    fn length(&self) -> Length;
    fn prot_flags(&self) -> ProtFlags;
    fn map_flags(&self) -> MapFlags;
    fn object(&self) -> &Object;
}

impl<'a> ViewImpl for View<'a> {
    fn offset(&self) -> Offset {
        self.offset
    }

    fn length(&self) -> Length {
        self.length
    }

    fn prot_flags(&self) -> ProtFlags {
        let mut prot_flags = ProtFlags::PROT_READ;
        if self.execute {
            prot_flags |= ProtFlags::PROT_EXEC;
        }
        prot_flags
    }

    fn map_flags(&self) -> MapFlags {
        MapFlags::MAP_SHARED
    }

    fn object(&self) -> &Object {
        self.object
    }
}

impl<'a> ViewImpl for ViewMut<'a> {
    fn offset(&self) -> Offset {
        self.offset
    }

    fn length(&self) -> Length {
        self.length
    }

    fn prot_flags(&self) -> ProtFlags {
        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE
    }

    fn map_flags(&self) -> MapFlags {
        if self.copy_on_write {
            MapFlags::MAP_PRIVATE
        } else {
            MapFlags::MAP_SHARED
        }
    }

    fn object(&self) -> &Object {
        self.object
    }
}

#[derive(Debug)]
pub struct Object {
    fd: c_int,
    execute: bool,
}

impl Drop for Object {
    fn drop(&mut self) {
        let _ = close(self.fd);
    }
}

impl Object {
    pub fn anonymous(size: usize, execute: bool) -> Result<Self, Error> {
        Ok(Object {
            fd: open_anonymous(size.try_into().unwrap())?,
            execute,
        })
    }

    pub unsafe fn with_file(
        file: &std::fs::File,
        _size: u64,
        write: bool,
        execute: bool,
    ) -> Result<Self, Error> {
        let file = file.try_clone()?;
        let mapped = Object {
            fd: std::os::unix::io::IntoRawFd::into_raw_fd(file),
            execute,
        };

        // Check permissions for the "write" permission:
        // * The file must be opened read-write
        // * We cannot write to a file opened in append-mode with mmap
        let oflags =
            OFlag::from_bits(fcntl(mapped.fd, FcntlArg::F_GETFL).map_err(to_io_error)?).unwrap();
        let opened_correctly = if write {
            oflags.contains(OFlag::O_RDONLY) || oflags.contains(OFlag::O_RDWR)
        } else {
            oflags.contains(OFlag::O_RDWR) && !oflags.contains(OFlag::O_APPEND)
        };
        if opened_correctly {
            Ok(mapped)
        } else {
            Err(access_denied())
        }
    }
}

unsafe fn map_impl<T: ViewImpl>(ptr: *mut u8, view: &T) -> Result<*mut u8, Error> {
    mmap(
        ptr as *mut _,
        view.length().into(),
        view.prot_flags(),
        view.map_flags(),
        view.object().fd,
        u64::from(view.offset()).try_into().unwrap(),
    )
    .map(|x| x as *mut u8)
    .map_err(to_io_error)
}

fn map_multiple_impl<T: ViewImpl>(views: &[T]) -> Result<(*mut u8, usize), Error> {
    // Allocate mapping
    let len = views
        .iter()
        .fold(0, |length, view| length + usize::from(view.length()));
    let ptr = {
        let fd = open_anonymous(len.try_into().unwrap())?;
        // Safety: pointer is selected by kernel
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
        close(fd).map_err(to_io_error)?;
        ptr.map(|ptr| ptr as *mut u8).map_err(to_io_error)
    }?;

    let mut offset = 0;
    for view in views {
        // Safety: pointer is within previously allocated range
        unsafe {
            map_impl(ptr.add(offset), view)?;
        }
        offset += usize::from(view.length());
    }
    Ok((ptr, len))
}

pub fn map(view: &View<'_>) -> Result<(*const u8, usize), Error> {
    Ok((
        // Safety: the pointer is selected by the kernel.
        unsafe { map_impl(std::ptr::null_mut(), view)? as *const u8 },
        view.length().into(),
    ))
}

pub fn map_mut(view: &ViewMut<'_>) -> Result<(*mut u8, usize), Error> {
    Ok((
        // Safety: the pointer is selected by the kernel.
        unsafe { map_impl(std::ptr::null_mut(), view)? },
        view.length().into(),
    ))
}

pub fn map_multiple(views: &[View<'_>]) -> Result<(*const u8, usize), Error> {
    map_multiple_impl(views).map(|(ptr, len)| (ptr as *const u8, len))
}

pub fn map_multiple_mut(views: &[ViewMut<'_>]) -> Result<(*mut u8, usize), Error> {
    map_multiple_impl(views)
}

pub unsafe fn unmap(ptr: *mut u8, view_lengths: impl Iterator<Item = usize>) {
    munmap(ptr as *mut _, view_lengths.sum()).unwrap();
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
