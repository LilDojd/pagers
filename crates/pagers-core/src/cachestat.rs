//! Linux cachestat(2) syscall wrapper (kernel 6.5+).

use std::sync::LazyLock;

pub static SUPPORTED: LazyLock<bool> = LazyLock::new(probe_support);

#[cfg(target_os = "linux")]
fn probe_support() -> bool {
    use nix::errno::Errno;

    let mut cs = Cachestat::zeroed();
    let range = CachestatRange { off: 0, len: 0 };
    // SAFETY: repr(C) structs on the stack, invalid fd — no side effects.
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
}

#[cfg(not(target_os = "linux"))]
fn probe_support() -> bool {
    false
}

#[cfg(target_os = "linux")]
use internals::*;

#[cfg(target_os = "linux")]
mod internals {
    /// 451 on x86_64, aarch64, arm, riscv64, powerpc64, s390x.
    pub const SYS_CACHESTAT: libc::c_long = 451;

    #[repr(C)]
    pub struct CachestatRange {
        pub off: u64,
        pub len: u64,
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
        pub fn zeroed() -> Self {
            Self {
                nr_cache: 0,
                nr_dirty: 0,
                nr_writeback: 0,
                nr_evicted: 0,
                nr_recently_evicted: 0,
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub fn cached_pages(
    fd: std::os::unix::io::BorrowedFd<'_>,
    offset: u64,
    len: u64,
) -> nix::Result<u64> {
    use nix::errno::Errno;
    use std::os::unix::io::AsRawFd;

    let range = CachestatRange { off: offset, len };
    let mut cs = Cachestat::zeroed();
    // SAFETY: repr(C) structs on the stack, valid fd from caller.
    let ret = unsafe {
        libc::syscall(
            SYS_CACHESTAT,
            fd.as_raw_fd(),
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

    #[test]
    fn test_supported_returns_bool() {
        let _ = *SUPPORTED;
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use super::*;
        use std::io::Write;
        use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd};

        #[test]
        fn test_cached_pages_on_temp_file() {
            if !*SUPPORTED {
                eprintln!("cachestat not supported on this kernel, skipping");
                return;
            }

            let mut f = tempfile::NamedTempFile::new().unwrap();
            let page_size = *crate::pagesize::PAGE_SIZE;
            let data = vec![0xABu8; page_size * 4];
            f.write_all(&data).unwrap();
            f.flush().unwrap();

            let fd = f.as_file().as_fd();
            let pages = cached_pages(fd, 0, (page_size * 4) as u64).unwrap();
            assert!(pages > 0 && pages <= 4, "expected 1-4 pages, got {pages}");
        }

        #[test]
        fn test_cached_pages_bad_fd() {
            if !*SUPPORTED {
                return;
            }
            let f = tempfile::NamedTempFile::new().unwrap();
            let fd = f.as_file().as_fd();
            let raw = fd.as_raw_fd();
            drop(f);
            let bad_fd = unsafe { BorrowedFd::borrow_raw(raw) };
            let err = cached_pages(bad_fd, 0, 0).unwrap_err();
            assert_eq!(err, nix::errno::Errno::EBADF);
        }
    }
}
