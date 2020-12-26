use super::*;
use crate::error::access_denied;
use std::convert::TryInto;
use winapi::{
    shared::minwindef::DWORD,
    um::{
        handleapi::INVALID_HANDLE_VALUE,
        memoryapi::{
            CreateFileMappingW, MapViewOfFileEx, UnmapViewOfFile, VirtualAlloc, VirtualFree,
            FILE_MAP_ALL_ACCESS, FILE_MAP_COPY, FILE_MAP_EXECUTE, FILE_MAP_READ,
        },
        sysinfoapi::{GetSystemInfo, SYSTEM_INFO},
        winnt::{
            HANDLE, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
            PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, SEC_COMMIT,
        },
    },
};

impl ViewPermissions {
    fn access_flags(&self) -> DWORD {
        match self {
            Self::Read => FILE_MAP_READ,
            Self::Write => FILE_MAP_ALL_ACCESS,
            Self::CopyOnWrite => FILE_MAP_READ | FILE_MAP_COPY,
            Self::Execute => FILE_MAP_EXECUTE,
        }
    }
}

#[derive(Debug)]
pub struct MappedObject {
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

impl MappedObject {
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
        permissions: FilePermissions,
    ) -> Result<Self, Error> {
        let access = match permissions {
            FilePermissions::Read => PAGE_READONLY,
            FilePermissions::Write => PAGE_READWRITE,
            FilePermissions::Execute => PAGE_EXECUTE_READ,
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

    unsafe fn map_impl(&self, ptr: *mut u8, view: &ViewOptions) -> Result<*mut u8, Error> {
        let (offset_hi, offset_lo) = split_dword(view.offset);
        let addr = MapViewOfFileEx(
            self.handle,
            view.permissions.access_flags(),
            offset_hi,
            offset_lo,
            view.length,
            ptr as *mut _,
        );
        if addr.is_null() {
            Err(Error::last_os_error())
        } else {
            Ok(addr as *mut u8)
        }
    }

    unsafe fn map(&self, view: &ViewOptions) -> Result<*mut u8, Error> {
        self.map_impl(std::ptr::null_mut(), view)
    }

    unsafe fn map_hint(&self, ptr: *mut u8, view: &ViewOptions) -> Result<(), Error> {
        self.map_impl(ptr as *mut _, view).map(std::mem::drop)
    }
}

fn system_info() -> SYSTEM_INFO {
    unsafe {
        let mut system_info = std::mem::MaybeUninit::<SYSTEM_INFO>::uninit();
        GetSystemInfo(system_info.as_mut_ptr());
        system_info.assume_init()
    }
}

pub fn offset_granularity() -> u64 {
    system_info().dwPageSize.try_into().unwrap()
}

pub fn length_granularity() -> usize {
    system_info().dwAllocationGranularity.try_into().unwrap()
}

pub fn allocate<'a>(views: &[View<'a>], mutable: bool) -> Result<*mut u8, Error> {
    if mutable && !views.iter().all(|view| view.is_mutable()) {
        return Err(access_denied());
    }

    // Allocate mapping
    let len = views.iter().fold(0usize, |len, view| {
        len.checked_add(view.options.length).unwrap()
    });

    let try_map = || {
        let ptr = unsafe {
            let ptr = VirtualAlloc(std::ptr::null_mut(), len, MEM_RESERVE, PAGE_NOACCESS);
            if ptr.is_null() || VirtualFree(ptr, 0, MEM_RELEASE) == 0 {
                return Err(Error::last_os_error());
            }
            ptr as *mut u8
        };

        let mut offset = 0;
        for (i, view) in views.iter().enumerate() {
            unsafe {
                view.mapping
                    .map_hint(ptr.add(offset), &view.options)
                    .map_err(|e| {
                        let _ = deallocate(ptr, views[..i].iter().map(|view| view.options.length));
                        e
                    })?;
            }
            offset += view.options.length;
        }
        Ok(ptr)
    };

    let mut tries = 0;
    const MAX_TRIES: usize = 10;
    loop {
        tries += 1;
        match try_map() {
            Ok(ptr) => break Ok(ptr),
            Err(err) => {
                if tries == MAX_TRIES {
                    break Err(err);
                } else {
                    continue;
                }
            }
        }
    }
}

pub unsafe fn deallocate(
    map: *mut u8,
    view_lengths: impl Iterator<Item = usize>,
) -> Result<(), Error> {
    let mut result = Ok(());
    let mut offset = 0;
    for length in view_lengths {
        if UnmapViewOfFile(map.add(offset) as *const _) == 0 && result.is_ok() {
            result = Err(Error::last_os_error());
        }
        offset += length;
    }
    result
}
