use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("encountered a race condition")]
    RaceCondition,
    #[error("{0}")]
    SystemError(#[from] std::io::Error),
}

#[cfg(unix)]
impl From<nix::Error> for Error {
    fn from(error: nix::Error) -> Self {
        if let Some(errno) = error.as_errno() {
            if errno != nix::errno::Errno::UnknownErrno {
                let value: i32 = unsafe { std::mem::transmute(errno) };
                return std::io::Error::from_raw_os_error(value).into();
            }
        }
        std::io::Error::new(std::io::ErrorKind::Other, error).into()
    }
}

impl Error {
    pub(crate) fn convert_size(size: u64) -> Result<usize, Self> {
        use std::convert::TryInto;
        size.try_into().map_err(|_| {
            std::io::Error::from_raw_os_error(
                #[cfg(unix)]
                {
                    nix::libc::EOVERFLOW
                },
                #[cfg(windows)]
                {
                    winapi::shared::winerror::ERROR_NOT_ENOUGH_MEMORY
                        .try_into()
                        .unwrap()
                },
            )
            .into()
        })
    }
}
