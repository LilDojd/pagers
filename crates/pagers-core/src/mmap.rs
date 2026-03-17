//! Safe wrappers around mmap, mincore, madvise.

use memmap2::{Advice, Mmap};

/// Query page residency for an mmap'd region.
/// Returns a Vec<bool> with one entry per page (true = resident).
pub fn mincore_residency(mmap: &Mmap, len: usize) -> std::io::Result<Vec<bool>> {
    let page_size = page_size();
    let num_pages = len.div_ceil(page_size);
    let mut vec: Vec<u8> = vec![0u8; num_pages];

    let ret = unsafe {
        libc::mincore(
            mmap.as_ptr() as *mut libc::c_void,
            len,
            vec.as_mut_ptr() as *mut libc::c_char,
        )
    };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(vec.into_iter().map(|b| b & 1 != 0).collect())
}

/// Returns the system page size.
pub fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

/// Issue madvise(MADV_WILLNEED) on a range of the mmap.
pub fn advise_willneed(mmap: &Mmap, offset: usize, len: usize) -> std::io::Result<()> {
    mmap.advise_range(Advice::WillNeed, offset, len)
}

/// Evict pages using posix_fadvise (Linux only — macOS uses msync at ops layer).
#[cfg(target_os = "linux")]
pub fn evict(fd: i32, offset: i64, len: i64) -> std::io::Result<()> {
    let ret = unsafe { libc::posix_fadvise(fd, offset, len, libc::POSIX_FADV_DONTNEED) };
    if ret != 0 {
        return Err(std::io::Error::from_raw_os_error(ret));
    }
    Ok(())
}

/// Lock pages in physical memory.
pub fn mlock(mmap: &Mmap, len: usize) -> std::io::Result<()> {
    let ret = unsafe { libc::mlock(mmap.as_ptr() as *const libc::c_void, len) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EPERM) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "mlock failed — may need SYS_IPC_LOCK capability or higher memory limits",
            ));
        }
        return Err(err);
    }
    Ok(())
}

/// Call mlockall(MCL_CURRENT) to lock all current mappings.
pub fn mlockall_current() -> std::io::Result<()> {
    let ret = unsafe { libc::mlockall(libc::MCL_CURRENT) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EPERM) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "mlockall failed — may need SYS_IPC_LOCK capability or higher memory limits",
            ));
        }
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use memmap2::MmapOptions;
    use std::io::Write;

    fn create_temp_file(size: usize) -> (tempfile::NamedTempFile, Mmap) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let data = vec![0xABu8; size];
        f.write_all(&data).unwrap();
        f.flush().unwrap();

        let mmap = unsafe { MmapOptions::new().map(f.as_file()).unwrap() };
        (f, mmap)
    }

    #[test]
    fn test_page_size_is_positive() {
        assert!(page_size() > 0);
        assert!(page_size().is_power_of_two());
    }

    #[test]
    fn test_mincore_returns_correct_page_count() {
        let ps = page_size();
        let size = ps * 4;
        let (_f, mmap) = create_temp_file(size);

        let residency = mincore_residency(&mmap, size).unwrap();
        assert_eq!(residency.len(), 4);
    }

    #[test]
    fn test_advise_willneed_succeeds() {
        let ps = page_size();
        let size = ps * 4;
        let (_f, mmap) = create_temp_file(size);

        advise_willneed(&mmap, 0, size).unwrap();
    }

    #[test]
    fn test_mincore_after_touch_shows_resident() {
        let ps = page_size();
        let size = ps * 4;
        let (_f, mmap) = create_temp_file(size);

        // Touch all pages by reading them
        let mut junk: u8 = 0;
        for i in 0..4 {
            junk = junk.wrapping_add(mmap[i * ps]);
        }
        let _ = junk;

        let residency = mincore_residency(&mmap, size).unwrap();
        assert!(residency.iter().all(|&r| r));
    }
}
