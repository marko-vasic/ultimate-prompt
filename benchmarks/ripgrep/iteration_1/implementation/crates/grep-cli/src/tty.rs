/// Terminal (TTY) detection utilities.
///
/// These functions detect whether standard I/O streams are connected to a
/// terminal, which is useful for deciding whether to use colors or other
/// interactive features.

/// Returns `true` if stdout is connected to a terminal (TTY).
///
/// On Unix, this uses `libc::isatty`. On other platforms, this always
/// returns `false`.
pub fn is_tty_stdout() -> bool {
    is_tty(imp::STDOUT_FD)
}

/// Returns `true` if stderr is connected to a terminal (TTY).
///
/// On Unix, this uses `libc::isatty`. On other platforms, this always
/// returns `false`.
pub fn is_tty_stderr() -> bool {
    is_tty(imp::STDERR_FD)
}

/// Returns `true` if stdin is connected to a terminal (TTY).
///
/// On Unix, this uses `libc::isatty`. On other platforms, this always
/// returns `false`.
pub fn is_tty_stdin() -> bool {
    is_tty(imp::STDIN_FD)
}

#[cfg(unix)]
fn is_tty(fd: libc::c_int) -> bool {
    // SAFETY: isatty is safe to call with any file descriptor value.
    unsafe { libc::isatty(fd) != 0 }
}

#[cfg(not(unix))]
fn is_tty(_fd: i32) -> bool {
    false
}

#[cfg(unix)]
mod imp {
    pub const STDIN_FD: libc::c_int = libc::STDIN_FILENO;
    pub const STDOUT_FD: libc::c_int = libc::STDOUT_FILENO;
    pub const STDERR_FD: libc::c_int = libc::STDERR_FILENO;
}

#[cfg(not(unix))]
mod imp {
    pub const STDIN_FD: i32 = 0;
    pub const STDOUT_FD: i32 = 1;
    pub const STDERR_FD: i32 = 2;
}
