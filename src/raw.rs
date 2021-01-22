//! Functions for allocating and deallocating virtual memory.

use crate::map_impl;
use crate::view::{View, ViewMut};
use std::io::Error;

/// Map a view of an object to memory.
///
/// Returns a tuple containing the memory map and the size of the map.
pub fn map(view: &View<'_>) -> Result<(*const u8, usize), Error> {
    map_impl::map(view)
}

/// Map a mutable view of an object to memory.
///
/// Returns a tuple containing the memory map and the size of the map.
pub fn map_mut(view: &ViewMut<'_>) -> Result<(*mut u8, usize), Error> {
    map_impl::map_mut(view)
}

/// Map views of objects contiguously to memory.
///
/// Returns a tuple containing the memory map and the size of the map.
pub fn map_multiple(views: &[View<'_>]) -> Result<(*const u8, usize), Error> {
    map_impl::map_multiple(views)
}

/// Map mutable views of objects contiguously to memory.
///
/// Returns a tuple containing the memory map and the size of the map.
pub fn map_multiple_mut(views: &[ViewMut<'_>]) -> Result<(*mut u8, usize), Error> {
    map_impl::map_multiple_mut(views)
}

/// Unmap a memory map.
///
/// # Safety
/// * `ptr` must be a memory map allocated with one of [`map`], [`map_mut`], [`map_multiple`], or
/// [`map_multiple_mut`].
/// * `view_lengths` must produce the lengths of each view in the memory map.
pub unsafe fn unmap(ptr: *mut u8, view_lengths: impl Iterator<Item = usize>) {
    map_impl::unmap(ptr, view_lengths)
}
