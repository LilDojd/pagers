use std::ptr::NonNull;

use memmap2::Mmap;
use nix::errno::Errno;
use nix::sys::mman::{MlockAllFlags, mlockall};

fn eperm_message(call: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!("{call} failed — may need SYS_IPC_LOCK capability or higher memory limits"),
    )
}

pub fn mlock(mmap: &Mmap, len: usize) -> std::io::Result<()> {
    // SAFETY: mmap.as_ptr() points to a valid memory-mapped region of at least `len` bytes.
    let addr = NonNull::new(mmap.as_ptr() as *mut std::ffi::c_void)
        .expect("mmap pointer should never be null");
    unsafe { nix::sys::mman::mlock(addr, len) }.map_err(|e| match e {
        Errno::EPERM => eperm_message("mlock"),
        other => other.into(),
    })
}

pub fn mlockall_current() -> std::io::Result<()> {
    mlockall(MlockAllFlags::MCL_CURRENT).map_err(|e| match e {
        Errno::EPERM => eperm_message("mlockall"),
        other => other.into(),
    })
}
