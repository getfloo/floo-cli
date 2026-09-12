#[cfg(unix)]
pub fn stdout_terminal() -> (std::fs::File, std::fs::File) {
    use std::os::fd::FromRawFd;
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: openpty receives valid output pointers and null optional settings.
    // On success each fresh descriptor is transferred to exactly one File.
    unsafe {
        assert_eq!(
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            0
        );
        (
            std::fs::File::from_raw_fd(master),
            std::fs::File::from_raw_fd(slave),
        )
    }
}
