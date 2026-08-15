/// Process hardening: disable core dumps on macOS.
pub fn harden_process() {
    // Disable core dumps.
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

/// Lock a memory region to prevent it from being swapped to disk.
/// Safety: ptr must be valid for len bytes.
#[allow(dead_code)]
pub fn mlock(ptr: *const u8, len: usize) -> bool {
    unsafe { libc::mlock(ptr as *const libc::c_void, len) == 0 }
}

/// Unlock a previously locked memory region.
/// Safety: ptr must be valid for len bytes and previously locked.
#[allow(dead_code)]
pub fn munlock(ptr: *const u8, len: usize) -> bool {
    unsafe { libc::munlock(ptr as *const libc::c_void, len) == 0 }
}
