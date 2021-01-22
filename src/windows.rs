use crate::view::{Length, Offset, View, ViewMut};
use std::convert::TryInto;
use std::io::Error;
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

trait ViewImpl {
    fn offset(&self) -> Offset;
    fn length(&self) -> Length;
    fn access_flags(&self) -> DWORD;
    fn object(&self) -> &Object;
}

impl<'a> ViewImpl for View<'a> {
    fn offset(&self) -> Offset {
        self.offset
    }

    fn length(&self) -> Length {
        self.length
    }

    fn access_flags(&self) -> DWORD {
        if self.execute {
            FILE_MAP_EXECUTE
        } else {
            FILE_MAP_READ
        }
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

    fn access_flags(&self) -> DWORD {
        if self.copy_on_write {
            FILE_MAP_READ | FILE_MAP_COPY
        } else {
            FILE_MAP_ALL_ACCESS
        }
    }

    fn object(&self) -> &Object {
        self.object
    }
}

#[derive(Debug)]
pub struct Object {
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

impl Object {
    pub fn anonymous(size: usize, execute: bool) -> Result<Self, Error> {
        let access = if execute {
            PAGE_EXECUTE_READWRITE | SEC_COMMIT
        } else {
            PAGE_READWRITE | SEC_COMMIT
        };
        let (size_hi, size_lo) = split_dword(size);
        // Safety:
        // Fulfills API expectations.
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
        write: bool,
        execute: bool,
    ) -> Result<Self, Error> {
        let access = match (write, execute) {
            (false, false) => PAGE_READONLY,
            (true, false) => PAGE_READWRITE,
            (false, true) => PAGE_EXECUTE_READ,
            (true, true) => PAGE_EXECUTE_READWRITE,
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
}

fn system_info() -> SYSTEM_INFO {
    // Safety:
    // system_info is always initialized by GetSystemInfo
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

// Must take care with the pointer provided.  The pointer must be null, or must point to a
// reserved virtual memory region that was previously allocated and freed.
unsafe fn map_impl<T: ViewImpl>(ptr: *mut u8, view: &T) -> Result<*mut u8, Error> {
    let (offset_hi, offset_lo) = split_dword(u64::from(view.offset()));
    let addr = MapViewOfFileEx(
        view.object().handle,
        view.access_flags(),
        offset_hi,
        offset_lo,
        view.length().into(),
        ptr as *mut _,
    );
    if addr.is_null() {
        Err(Error::last_os_error())
    } else {
        Ok(addr as *mut u8)
    }
}

fn map_multiple_impl<T: ViewImpl>(views: &[T]) -> Result<(*mut u8, usize), Error> {
    // Allocate mapping
    let len = views
        .into_iter()
        .fold(0, |length, view| length + usize::from(view.length()));
    let try_map = || {
        // Safety:
        // Pointer is either an available memory region or null. We only deallocate memory
        // that we immediately allocated.
        let ptr = unsafe {
            let ptr = VirtualAlloc(std::ptr::null_mut(), len, MEM_RESERVE, PAGE_NOACCESS);
            if ptr.is_null() || VirtualFree(ptr, 0, MEM_RELEASE) == 0 {
                return Err(Error::last_os_error());
            }
            ptr as *mut u8
        };

        let mut offset = 0;
        for (i, view) in views.iter().enumerate() {
            // Safety:
            // The pointer is the next available memory region.
            unsafe {
                if let Err(err) = map_impl(ptr.add(offset), view) {
                    unmap(ptr, views[..i].iter().map(|v| v.length().into()));
                    return Err(err);
                }
            }
            offset += usize::from(view.length());
        }
        Ok(ptr)
    };

    let mut tries = 0;
    const MAX_TRIES: usize = 10;
    loop {
        tries += 1;
        match try_map() {
            Ok(ptr) => break Ok((ptr, len)),
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

pub fn map(view: &View<'_>) -> Result<(*const u8, usize), Error> {
    Ok((
        // Safety: the pointer is selected by the kernel.
        unsafe { map_impl(std::ptr::null_mut(), view)? as *const u8 },
        view.length.into(),
    ))
}

pub fn map_mut(view: &ViewMut<'_>) -> Result<(*mut u8, usize), Error> {
    Ok((
        // Safety: the pointer is selected by the kernel.
        unsafe { map_impl(std::ptr::null_mut(), view)? },
        view.length.into(),
    ))
}

pub fn map_multiple(views: &[View<'_>]) -> Result<(*const u8, usize), Error> {
    map_multiple_impl(views).map(|(ptr, len)| (ptr as *const u8, len))
}

pub fn map_multiple_mut(views: &[ViewMut<'_>]) -> Result<(*mut u8, usize), Error> {
    map_multiple_impl(views)
}

pub unsafe fn unmap(mut ptr: *mut u8, view_lengths: impl Iterator<Item = usize>) {
    for l in view_lengths {
        let status = UnmapViewOfFile(ptr as *const _);
        debug_assert!(status != 0);
        ptr = ptr.add(l);
    }
}
