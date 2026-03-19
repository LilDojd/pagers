//! Safe wrappers around mmap, mincore, madvise.

use std::ptr::NonNull;

use memmap2::{Advice, Mmap};
use nix::errno::Errno;
use nix::sys::mman::{mlockall, MlockAllFlags};

/// Query page residency for an mmap'd region.
/// Returns a Vec<bool> with one entry per page (true = resident).
pub fn mincore_residency(mmap: &Mmap, len: usize) -> nix::Result<Vec<bool>> {
    let page_size = page_size();
    // Size is from mincore man page
    let vec_len = (len + page_size - 1) / page_size;
    let mut vec_out: Vec<u8> = Vec::with_capacity(vec_len);

    unsafe {
        // SAFETY: mincore takes a pointer to a virtual memory region and writes
        // RAM residency information to the memory region at vec_out, with the
        // length computed above using the expression from the mincore man page.
        // We have allocated the underlying buffer by using with_capacity.
        if libc::mincore(
            mmap.as_ptr() as *mut libc::c_void,
            len,
            vec_out.as_mut_ptr(),
        ) != 0
        {
            // Returncode of either 0 (success) or -1 (failure, see errno)
            // We don't do any other calls in between mincore and Errno::last so errno is untouched
            // errno is thread-unique so there are no race conditions
            return Err(Errno::last());
        }
        // SAFETY: we just filled up the vector with valid values
        vec_out.set_len(vec_len);
    }
    Ok(vec_out.into_iter().map(|x| x != 0).collect())
}

/// Returns the system page size.
pub fn page_size() -> usize {
    usize::try_from(
        nix::unistd::sysconf(nix::unistd::SysconfVar::PAGE_SIZE)
            .expect("Failed to fetch _SC_PAGESIZE")
            .expect("_SC_PAGESIZE returned None"),
    )
    .unwrap()
}

/// Issue madvise(MADV_WILLNEED) on a range of the mmap.
pub fn advise_willneed(mmap: &Mmap, offset: usize, len: usize) -> std::io::Result<()> {
    mmap.advise_range(Advice::WillNeed, offset, len)
}

fn eperm_message(call: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!("{call} failed — may need SYS_IPC_LOCK capability or higher memory limits"),
    )
}

/// Lock pages in physical memory.
pub fn mlock(mmap: &Mmap, len: usize) -> std::io::Result<()> {
    // SAFETY: mmap.as_ptr() points to a valid memory-mapped region of at least `len` bytes.
    let addr = NonNull::new(mmap.as_ptr() as *mut std::ffi::c_void)
        .expect("mmap pointer should never be null");
    unsafe { nix::sys::mman::mlock(addr, len) }.map_err(|e| match e {
        Errno::EPERM => eperm_message("mlock"),
        other => other.into(),
    })
}

/// Call mlockall(MCL_CURRENT) to lock all current mappings.
pub fn mlockall_current() -> std::io::Result<()> {
    mlockall(MlockAllFlags::MCL_CURRENT).map_err(|e| match e {
        Errno::EPERM => eperm_message("mlockall"),
        other => other.into(),
    })
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
        let ps = page_size();
        assert!(ps > 0);
        assert!(ps.is_power_of_two());
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
