use std::ptr::NonNull;

use memmap2::Mmap;
use nix::errno::Errno;
use nix::sys::mman::{MlockAllFlags, mlockall};

use crate::error::MlockError;

pub fn mlock(mmap: &Mmap, len: usize) -> crate::Result<()> {
    // SAFETY: mmap.as_ptr() points to a valid memory-mapped region of at least `len` bytes.
    let addr = NonNull::new(mmap.as_ptr() as *mut std::ffi::c_void)
        .expect("mmap pointer should never be null");
    unsafe { nix::sys::mman::mlock(addr, len) }.map_err(|e| match e {
        Errno::EPERM => MlockError::PermissionDenied { call: "mlock" },
        Errno::ENOMEM => MlockError::OutOfMemory { call: "mlock", len },
        other => MlockError::Other {
            call: "mlock",
            source: other.into(),
        },
    })?;
    Ok(())
}

pub fn mlockall_current() -> crate::Result<()> {
    mlockall(MlockAllFlags::MCL_CURRENT).map_err(|e| match e {
        Errno::EPERM => MlockError::PermissionDenied { call: "mlockall" },
        Errno::ENOMEM => MlockError::OutOfMemory {
            call: "mlockall",
            len: 0,
        },
        other => MlockError::Other {
            call: "mlockall",
            source: other.into(),
        },
    })?;
    Ok(())
}
