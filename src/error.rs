/// The error code returned by the OS if something fails.
#[derive(Copy, Clone, Debug)]
pub struct Error(pub i32);

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "system error code: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl From<Error> for std::io::Error {
    fn from(e: Error) -> Self {
        Self::from_raw_os_error(e.0)
    }
}
