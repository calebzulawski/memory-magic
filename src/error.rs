pub(crate) fn access_denied() -> std::io::Error {
    std::io::Error::from_raw_os_error(
        #[cfg(unix)]
        {
            nix::libc::EACCES
        },
        #[cfg(windows)]
        {
            use std::convert::TryInto;
            winapi::shared::winerror::ERROR_ACCESS_DENIED
                .try_into()
                .unwrap()
        },
    )
}

#[cfg(unix)]
pub(crate) fn to_io_error(error: nix::Error) -> std::io::Error {
    if let Some(errno) = error.as_errno() {
        if errno != nix::errno::Errno::UnknownErrno {
            let value: i32 = unsafe { std::mem::transmute(errno) };
            return std::io::Error::from_raw_os_error(value);
        }
    }
    std::io::Error::new(std::io::ErrorKind::Other, error)
}
