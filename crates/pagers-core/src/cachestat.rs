//! Linux cachestat(2) syscall wrapper (kernel 6.5+).

use std::os::unix::io::RawFd;
use std::sync::OnceLock;

use nix::errno::Errno;

/// 451 on x86_64, aarch64, arm, riscv64, powerpc64, s390x.
const SYS_CACHESTAT: libc::c_long = 451;

#[repr(C)]
struct CachestatRange {
    off: u64,
    len: u64,
}

#[repr(C)]
pub struct Cachestat {
    pub nr_cache: u64,
    pub nr_dirty: u64,
    pub nr_writeback: u64,
    pub nr_evicted: u64,
    pub nr_recently_evicted: u64,
}

impl Cachestat {
    fn zeroed() -> Self {
        Self {
            nr_cache: 0,
            nr_dirty: 0,
            nr_writeback: 0,
            nr_evicted: 0,
            nr_recently_evicted: 0,
        }
    }
}

/// Probes once on first call whether the kernel supports cachestat(2).
pub fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        // Probe with an invalid fd (-1). If the syscall exists, we get
        // EBADF or EFAULT. If it doesn't exist, we get ENOSYS.
        let mut cs = Cachestat::zeroed();
        let range = CachestatRange { off: 0, len: 0 };
        let ret = unsafe {
            libc::syscall(
                SYS_CACHESTAT,
                -1i32,
                &range as *const CachestatRange,
                &mut cs as *mut Cachestat,
                0u32,
            )
        };
        if ret == -1 {
            Errno::last() != Errno::ENOSYS
        } else {
            true
        }
    })
}

/// Returns pages in cache for `[offset, offset+len)` of `fd`.
pub fn cached_pages(fd: RawFd, offset: u64, len: u64) -> nix::Result<u64> {
    let range = CachestatRange { off: offset, len };
    let mut cs = Cachestat::zeroed();
    let ret = unsafe {
        libc::syscall(
            SYS_CACHESTAT,
            fd,
            &range as *const CachestatRange,
            &mut cs as *mut Cachestat,
            0u32,
        )
    };
    if ret == 0 {
        Ok(cs.nr_cache)
    } else {
        Err(Errno::last())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::io::AsRawFd;

    #[test]
    fn test_supported_returns_bool() {
        let _ = supported();
    }

    #[test]
    fn test_cached_pages_on_temp_file() {
        if !supported() {
            eprintln!("cachestat not supported on this kernel, skipping");
            return;
        }

        let mut f = tempfile::NamedTempFile::new().unwrap();
        let page_size = crate::mmap::page_size();
        let data = vec![0xABu8; page_size * 4];
        f.write_all(&data).unwrap();
        f.flush().unwrap();

        let fd = f.as_file().as_raw_fd();
        let pages = cached_pages(fd, 0, (page_size * 4) as u64).unwrap();
        assert!(pages > 0 && pages <= 4, "expected 1-4 pages, got {pages}");
    }

    #[test]
    fn test_cached_pages_bad_fd() {
        if !supported() {
            return;
        }
        let err = cached_pages(-1, 0, 0).unwrap_err();
        assert_eq!(err, Errno::EBADF);
    }
}
