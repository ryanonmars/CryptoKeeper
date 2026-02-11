/// Process hardening: disable core dumps and ptrace on supported platforms.
pub fn harden_process() {
    #[cfg(target_os = "linux")]
    {
        // Disable core dumps
        unsafe {
            libc::setrlimit(
                libc::RLIMIT_CORE,
                &libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                },
            );
        }
        // Disable ptrace
        unsafe {
            libc::prctl(libc::PR_SET_DUMPABLE, 0);
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Disable core dumps
        unsafe {
            libc::setrlimit(
                libc::RLIMIT_CORE,
                &libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                },
            );
        }
    }
}

/// Lock a memory region to prevent it from being swapped to disk.
/// Safety: ptr must be valid for len bytes.
#[allow(dead_code)]
#[cfg(unix)]
pub fn mlock(ptr: *const u8, len: usize) -> bool {
    unsafe { libc::mlock(ptr as *const libc::c_void, len) == 0 }
}

#[cfg(not(unix))]
pub fn mlock(_ptr: *const u8, _len: usize) -> bool {
    false
}

/// Unlock a previously locked memory region.
/// Safety: ptr must be valid for len bytes and previously locked.
#[allow(dead_code)]
#[cfg(unix)]
pub fn munlock(ptr: *const u8, len: usize) -> bool {
    unsafe { libc::munlock(ptr as *const libc::c_void, len) == 0 }
}

#[cfg(not(unix))]
pub fn munlock(_ptr: *const u8, _len: usize) -> bool {
    false
}
