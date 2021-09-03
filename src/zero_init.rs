/// Types that can be initialized with all zeros.
///
/// # Safety
/// This trait can be implemented for any type where it is safe to `transmute` an array of zeros to
/// this type.
pub unsafe trait ZeroInit {}

unsafe impl ZeroInit for i8 {}
unsafe impl ZeroInit for i16 {}
unsafe impl ZeroInit for i32 {}
unsafe impl ZeroInit for i64 {}
unsafe impl ZeroInit for i128 {}
unsafe impl ZeroInit for isize {}

unsafe impl ZeroInit for u8 {}
unsafe impl ZeroInit for u16 {}
unsafe impl ZeroInit for u32 {}
unsafe impl ZeroInit for u64 {}
unsafe impl ZeroInit for u128 {}
unsafe impl ZeroInit for usize {}

unsafe impl ZeroInit for f32 {}
unsafe impl ZeroInit for f64 {}

unsafe impl<T> ZeroInit for *const T {}
unsafe impl<T> ZeroInit for *mut T {}
