use super::{FileAccess, FileOptions, MapOptions, PageAccess};
use crate::error::Error;
use std::convert::TryInto;
use winapi::{
    shared::minwindef::DWORD,
    um::{
        handleapi::INVALID_HANDLE_VALUE,
        memoryapi::{
            CreateFileMappingW, MapViewOfFileEx, FILE_MAP_ALL_ACCESS, FILE_MAP_COPY,
            FILE_MAP_EXECUTE, FILE_MAP_READ,
        },
        winnt::{
            HANDLE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_READONLY, PAGE_READWRITE,
            SEC_COMMIT,
        },
    },
};

impl MapOptions {
    fn access_flags(&self) -> DWORD {
        let mut flags = match self.access {
            PageAccess::Read => FILE_MAP_READ,
            PageAccess::Write => FILE_MAP_ALL_ACCESS,
            PageAccess::CopyOnWrite => FILE_MAP_READ | FILE_MAP_COPY,
        };
        if self.executable {
            flags |= FILE_MAP_EXECUTE;
        }
        flags
    }
}

pub struct Mapping {
    handle: HANDLE,
}

fn split_dword<T>(value: T) -> (DWORD, DWORD)
where
    T: num_traits::Zero + num_traits::CheckedShr + std::ops::BitAnd<Output = T> + TryInto<DWORD>,
    <T as TryInto<DWORD>>::Error: std::fmt::Debug,
    DWORD: TryInto<T>,
    <DWORD as TryInto<T>>::Error: std::fmt::Debug,
{
    (
        value
            .checked_shr(std::mem::size_of::<DWORD>() as DWORD * 8)
            .unwrap_or(T::zero())
            .try_into()
            .unwrap(),
        (value & DWORD::MAX.try_into().unwrap()).try_into().unwrap(),
    )
}

impl Mapping {
    pub fn anonymous(size: usize, executable: bool) -> Result<Self, Error> {
        let access = if executable {
            PAGE_EXECUTE_READWRITE | SEC_COMMIT
        } else {
            PAGE_READWRITE | SEC_COMMIT
        };
        let (size_hi, size_lo) = split_dword(size);
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null_mut(),
                access,
                size_hi,
                size_lo,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(Self { handle })
        }
    }

    pub unsafe fn with_file(
        file: &std::fs::File,
        size: u64,
        options: &FileOptions,
    ) -> Result<Self, Error> {
        let access = match (options.executable, options.access) {
            (false, FileAccess::Read) => PAGE_READONLY,
            (false, FileAccess::Write) => PAGE_READWRITE,
            (true, FileAccess::Read) => PAGE_EXECUTE_READ,
            (true, FileAccess::Write) => PAGE_EXECUTE_READWRITE,
        };
        let (size_hi, size_lo) = split_dword(size);
        let handle = CreateFileMappingW(
            std::os::windows::io::AsRawHandle::as_raw_handle(file),
            std::ptr::null_mut(),
            access,
            size_hi,
            size_lo,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(Self { handle })
        }
    }

    unsafe fn map_impl(
        &self,
        ptr: *mut u8,
        offset: u64,
        size: usize,
        options: &MapOptions,
    ) -> Result<*mut u8, Error> {
        let (offset_hi, offset_lo) = split_dword(offset);
        let addr = MapViewOfFileEx(
            self.handle,
            options.access_flags(),
            offset_hi,
            offset_lo,
            size,
            ptr as *mut _,
        );
        if addr.is_null() {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(addr as *mut u8)
        }
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
        self.map_impl(ptr as *mut _, offset, size, options)
            .map(std::mem::drop)
    }
}
