use crate::error::ToIoError;
use std::convert::TryInto;
use std::fs::File;
use std::io::Error;

#[cfg(windows)]
use std::os::windows::{raw::HANDLE, io::{AsRawHandle, IntoRawHandle}};

#[cfg(unix)]
use std::os::unix::io::{RawFd, AsRawFd, IntoRawFd};

pub struct Handle(
    #[cfg(unix)] RawFd,
    #[cfg(windows)] HANDLE,
);

impl Drop for Handle {
    #[cfg(unix)]
    fn drop(&mut self) {
        nix::unistd::close(self.0).unwrap()
    }

    #[cfg(windows)]
    fn drop(&mut self) {
        assert!(unsafe { winapi::um::handleapi::CloseHandle(self.0) } != 0);
    }
}

#[cfg(unix)]
fn create_anonymous(size: u64) -> Result<Handle, Error> {
    let fd = shm_open_anonymous::shm_open_anonymous();
    if fd == -1 {
        return Err(Error::last_os_error());
    }
    nix::unistd::ftruncate(fd, size.try_into().unwrap()).to_io_error()?;
    unsafe { Ok(Handle(std::os::unix::io::FromRawFd::from_raw_fd(fd))) }
}

impl Handle {
    pub fn anonymous(size: u64) -> Result<Self, Error> {
        create_anonymous(size)
    }

    #[cfg(windows)]
    unsafe fn from_file_handle(handle: HANDLE) -> Self {
            CreateFileMappingA(handle, ptr::null_mut(), 0, 0, ptr::null())
    }

    pub unsafe fn from_file(file: File) -> Self {
        #[cfg(unix)]
        {
            Self(IntoRawFd::into_raw_fd(file))
        }
        #[cfg(windows)]
        {
            Self::from_file_handle(IntoRawHandle::into_raw_handle(file))
        }
    }
}
