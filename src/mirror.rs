use crate::raw::{
    map_multiple_mut, unmap, Length, Object, Offset, ReadPermissions, WritePermissions,
};
use std::io::Error;

fn allocate_mirror<T>(min_size: usize) -> Result<(*mut T, usize), Error> {
    let offset = Offset::exact(0).unwrap();
    let min_size = (min_size + 1) / 2;
    let length = Length::round_up(min_size * std::mem::size_of::<T>());
    let object = Object::anonymous(length.to_usize(), ReadPermissions::Read)?;
    let view = object
        .view_mut(offset, length, WritePermissions::Write)
        .unwrap();
    map_multiple_mut(&[view; 2]).map(|(p, l)| (p as *mut T, l / std::mem::size_of::<T>()))
}

/// A mirrored memory region.
///
/// Changes in the first half of the slice propagates to the second half, and vice versa.
pub struct Mirror<T> {
    map: *mut T,
    len: usize,
}

impl<T> Drop for Mirror<T> {
    fn drop(&mut self) {
        let ptr = self.map as *mut u8;
        let len = self.len * std::mem::size_of::<T>();
        unsafe { unmap(ptr, [len / 2; 2].iter().copied()) }
    }
}

impl<T> Mirror<T> {
    unsafe fn new<F: Fn() -> T>(min_size: usize, value: Option<F>) -> Result<Self, Error> {
        let (map, len) = allocate_mirror::<T>(min_size)?;
        if let Some(value) = value {
            for i in 0..(len / 2) {
                map.add(i).write(value())
            }
        }
        Ok(Self { map, len })
    }
}

impl<T> Mirror<T>
where
    T: crate::ZeroInit,
{
    /// Initialize with zeroed values.
    ///
    /// The resulting slice has at least `min_size` elements.
    pub fn zeroed(min_size: usize) -> Result<Self, Error> {
        unsafe { Self::new::<fn() -> T>(min_size, None) }
    }
}

impl<T> Mirror<T>
where
    T: Default,
{
    /// Initialize with default values.
    ///
    /// The resulting slice has at least `min_size` elements.
    pub fn with_default(min_size: usize) -> Result<Self, Error> {
        unsafe { Self::new(min_size, Some(Default::default)) }
    }
}

impl<T> Mirror<T>
where
    T: Clone,
{
    /// Initialize with the provided value.
    ///
    /// The resulting slice has at least `min_size` elements.
    pub fn with_value(min_size: usize, value: T) -> Result<Self, Error> {
        unsafe { Self::new(min_size, Some(|| value.clone())) }
    }
}

impl<T> std::ops::Deref for Mirror<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { std::slice::from_raw_parts(self.map, self.len) }
    }
}

impl<T> std::ops::DerefMut for Mirror<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { std::slice::from_raw_parts_mut(self.map, self.len) }
    }
}

impl<T> AsRef<[T]> for Mirror<T> {
    fn as_ref(&self) -> &[T] {
        self
    }
}

impl<T> AsMut<[T]> for Mirror<T> {
    fn as_mut(&mut self) -> &mut [T] {
        self
    }
}

impl<T> std::borrow::Borrow<[T]> for Mirror<T> {
    fn borrow(&self) -> &[T] {
        self
    }
}

impl<T> std::borrow::BorrowMut<[T]> for Mirror<T> {
    fn borrow_mut(&mut self) -> &mut [T] {
        self
    }
}
