use crate::{
    view::{Length, Offset, View, ViewMut},
    Error,
};
use core::{convert::TryInto, num::NonZeroUsize};

fn errno() -> libc::c_int {
    #[cfg(any(target_os = "solaris", target_os = "illumos"))]
    use libc::___errno as errno_location;
    #[cfg(any(target_os = "android", target_os = "netbsd", target_os = "openbsd"))]
    use libc::__errno as errno_location;
    #[cfg(any(target_os = "linux", target_os = "redox", target_os = "dragonfly"))]
    use libc::__errno_location as errno_location;
    #[cfg(any(target_os = "freebsd", target_os = "ios", target_os = "macos"))]
    use libc::__error as errno_location;

    unsafe { *errno_location() as libc::c_int }
}

fn last_error() -> Error {
    Error(errno() as i32)
}

#[cfg(feature = "std")]
fn access_denied() -> Error {
    Error(libc::EACCES)
}

fn open_anonymous(size: i64) -> Result<libc::c_int, Error> {
    let fd = shm_open_anonymous::shm_open_anonymous();
    if fd == -1 {
        Err(last_error())
    } else {
        // Safety: fd is valid
        unsafe {
            if libc::ftruncate(fd, size) != 0 {
                let err = last_error();
                libc::close(fd);
                Err(err)
            } else {
                Ok(fd)
            }
        }
    }
}

trait ViewImpl {
    fn offset(&self) -> Offset;
    fn length(&self) -> Length;
    fn prot_flags(&self) -> libc::c_int;
    fn map_flags(&self) -> libc::c_int;
    fn object(&self) -> &Object;
}

impl<'a> ViewImpl for View<'a> {
    fn offset(&self) -> Offset {
        self.offset
    }

    fn length(&self) -> Length {
        self.length
    }

    fn prot_flags(&self) -> libc::c_int {
        let mut prot_flags = libc::PROT_READ;
        if self.execute {
            prot_flags |= libc::PROT_EXEC;
        }
        prot_flags
    }

    fn map_flags(&self) -> libc::c_int {
        libc::MAP_SHARED
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

    fn prot_flags(&self) -> libc::c_int {
        libc::PROT_READ | libc::PROT_WRITE
    }

    fn map_flags(&self) -> libc::c_int {
        if self.copy_on_write {
            libc::MAP_PRIVATE
        } else {
            libc::MAP_SHARED
        }
    }

    fn object(&self) -> &Object {
        self.object
    }
}

#[derive(Debug)]
pub struct Object {
    fd: libc::c_int,
    execute: bool,
}

impl Drop for Object {
    fn drop(&mut self) {
        // Safety: fd is valid
        unsafe {
            libc::close(self.fd);
        }
    }
}

impl Object {
    pub fn anonymous(size: usize, execute: bool) -> Result<Self, Error> {
        Ok(Object {
            fd: open_anonymous(size.try_into().unwrap())?,
            execute,
        })
    }

    #[cfg(feature = "std")]
    pub unsafe fn with_file(
        file: &std::fs::File,
        _size: u64,
        write: bool,
        execute: bool,
    ) -> Result<Self, Error> {
        let file = file
            .try_clone()
            .map_err(|e| Error(e.raw_os_error().unwrap_or(0)))?;
        let mapped = Object {
            fd: std::os::unix::io::IntoRawFd::into_raw_fd(file),
            execute,
        };

        // Check permissions for the "write" permission:
        // * The file must be opened read-write
        // * We cannot write to a file opened in append-mode with mmap
        let oflags = libc::fcntl(mapped.fd, libc::F_GETFL);
        if oflags == -1 {
            return Err(last_error());
        }
        let opened_correctly = if write {
            oflags == libc::O_RDONLY || oflags & libc::O_RDWR != 0
        } else {
            oflags & libc::O_RDWR != 0 && oflags & libc::O_APPEND == 0
        };
        if opened_correctly {
            Ok(mapped)
        } else {
            Err(access_denied())
        }
    }
}

unsafe fn map_impl<T: ViewImpl>(ptr: *mut u8, view: &T) -> Result<*mut u8, Error> {
    let mapped = libc::mmap(
        ptr as *mut _,
        view.length().into(),
        view.prot_flags(),
        view.map_flags(),
        view.object().fd,
        u64::from(view.offset()).try_into().unwrap(),
    );
    if mapped == libc::MAP_FAILED {
        Err(last_error())
    } else {
        Ok(mapped as *mut u8)
    }
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
            libc::mmap(
                core::ptr::null_mut(),
                len,
                libc::PROT_NONE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        let ptr = if ptr == libc::MAP_FAILED {
            Err(last_error())
        } else {
            Ok(ptr as *mut u8)
        };
        // Safety: fd is valid
        unsafe { libc::close(fd) };
        ptr
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
        unsafe { map_impl(core::ptr::null_mut(), view)? as *const u8 },
        view.length().into(),
    ))
}

pub fn map_mut(view: &ViewMut<'_>) -> Result<(*mut u8, usize), Error> {
    Ok((
        // Safety: the pointer is selected by the kernel.
        unsafe { map_impl(core::ptr::null_mut(), view)? },
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
    assert_eq!(libc::munmap(ptr as *mut _, view_lengths.sum()), 0);
}

fn page_size() -> u64 {
    // Safety: cannot fail
    let val = unsafe { libc::sysconf(libc::_SC_PAGE_SIZE) };
    assert!(val != -1);
    val.try_into().unwrap()
}

pub fn offset_granularity() -> NonZeroUsize {
    NonZeroUsize::new(page_size().try_into().unwrap()).unwrap()
}

pub fn length_granularity() -> NonZeroUsize {
    NonZeroUsize::new(page_size().try_into().unwrap()).unwrap()
}
