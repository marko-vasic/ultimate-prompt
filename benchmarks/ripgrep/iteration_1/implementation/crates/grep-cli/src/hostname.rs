/// Hostname detection utilities.
///
/// This module provides a cross-platform way to obtain the hostname
/// of the current machine.

/// Returns the hostname of the current machine, if it can be determined.
///
/// On Unix, this uses `libc::gethostname`. On other platforms, this
/// returns `None`.
pub fn hostname() -> Option<String> {
    imp::hostname()
}

#[cfg(unix)]
mod imp {
    use std::ffi::CStr;

    pub fn hostname() -> Option<String> {
        // Per POSIX, HOST_NAME_MAX is at least 255. We use a generous buffer.
        let mut buf = vec![0u8; 256];
        // SAFETY: We pass a valid buffer and its length.
        let ret = unsafe {
            libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len())
        };
        if ret != 0 {
            return None;
        }
        // Ensure null-termination in case the hostname fills the buffer.
        // gethostname may or may not null-terminate when truncating.
        if !buf.contains(&0) {
            return None;
        }
        // SAFETY: We ensured there's a null byte in the buffer.
        let cstr = unsafe {
            CStr::from_ptr(buf.as_ptr() as *const libc::c_char)
        };
        Some(cstr.to_string_lossy().into_owned())
    }
}

#[cfg(not(unix))]
mod imp {
    pub fn hostname() -> Option<String> {
        None
    }
}
