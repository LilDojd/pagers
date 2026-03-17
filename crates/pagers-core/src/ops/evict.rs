use std::fs::File;
use std::path::Path;

#[cfg(target_os = "linux")]
use std::os::unix::io::AsFd;

#[cfg(target_os = "linux")]
pub(crate) fn evict_fadvise(file: &File, offset: i64, len: i64) -> std::io::Result<()> {
    nix::fcntl::posix_fadvise(
        file.as_fd(),
        offset,
        len,
        nix::fcntl::PosixFadviseAdvice::POSIX_FADV_DONTNEED,
    )?;
    Ok(())
}

/// Evict pages from cache.
pub(crate) fn evict_file(file: &File, path: &Path, offset: i64, len: i64) -> anyhow::Result<()> {
    tracing::debug!("Evicting {}", path.display());

    #[cfg(target_os = "linux")]
    {
        evict_fadvise(file, offset, len)?;
    }

    #[cfg(target_os = "macos")]
    {
        use memmap2::MmapOptions;
        use nix::sys::mman::{MsFlags, msync};
        use std::ptr::NonNull;

        // SAFETY: `map` creates a valid memory-mapped region for the given file,
        // and we immediately derive a pointer from it. The pointer is non-null
        // and remains valid for the lifetime of `mmap` within this scope.
        // `len` matches the mapping length, and `offset`/`len` must be page-aligned
        // as required by the underlying OS. The mapping is not unmapped while
        // `msync` is called. The flags passed to `msync` follow its contract.
        unsafe {
            let mmap = MmapOptions::new()
                .offset(offset as u64)
                .len(len as usize)
                .map(file)?;
            let ptr =
                NonNull::new(mmap.as_ptr() as *mut _).expect("mmap pointer should be non-null");
            msync(ptr, len as usize, MsFlags::MS_INVALIDATE)?;
        }
    }

    Ok(())
}
