use std::fs::File;
use std::path::Path;

#[cfg(target_os = "linux")]
pub(crate) unsafe fn evict_fadvise(fd: i32, offset: i64, len: i64) -> std::io::Result<()> {
    // SAFETY: The caller guarantees that `fd` is a valid, open file descriptor.
    // `offset` and `len` must describe a valid range for the underlying file.
    let ret = libc::posix_fadvise(fd, offset, len, libc::POSIX_FADV_DONTNEED);

    if ret != 0 {
        return Err(std::io::Error::from_raw_os_error(ret));
    }

    Ok(())
}

/// Evict pages from cache.
pub(crate) fn evict_file(file: &File, path: &Path, offset: i64, len: i64) -> anyhow::Result<()> {
    tracing::debug!("Evicting {}", path.display());

    #[cfg(target_os = "linux")]
    unsafe {
        evict_fadvise(file.as_raw_fd(), offset, len)?;
    }

    #[cfg(target_os = "macos")]
    {
        use memmap2::MmapOptions;

        // On macOS, mmap + msync(MS_INVALIDATE)
        let mmap = unsafe {
            MmapOptions::new()
                .offset(offset as u64)
                .len(len as usize)
                .map(file)?
        };
        unsafe {
            if libc::msync(
                mmap.as_ptr() as *mut libc::c_void,
                len as usize,
                libc::MS_INVALIDATE,
            ) != 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }

    Ok(())
}
