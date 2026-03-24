use crate::mincore::PageMap;

use super::{FileContext, Op};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Evict;

impl Op for Evict {
    const LABEL: &str = "evicted";
    const SKIP_RESIDENCY: bool = true;
    const ACTION_SIGN: isize = -1;
    type Output = ();

    fn action_pages(
        _output: &(),
        total_pages: usize,
        pages_in_core_before: Option<usize>,
        pages_in_core_after: usize,
    ) -> usize {
        let before = pages_in_core_before.unwrap_or(total_pages);
        before.saturating_sub(pages_in_core_after)
    }

    fn execute<PM: PageMap + Sync>(&self, ctx: &FileContext<'_, PM>) -> crate::Result<()> {
        tracing::debug!("Evicting {}", ctx.path.display());

        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsFd;
            nix::fcntl::posix_fadvise(
                ctx.file.as_fd(),
                ctx.offset as libc::off_t,
                ctx.len as libc::off_t,
                nix::fcntl::PosixFadviseAdvice::POSIX_FADV_DONTNEED,
            )?;
        }

        #[cfg(target_os = "macos")]
        {
            use nix::sys::mman::{MsFlags, msync};
            use std::ptr::NonNull;

            unsafe {
                let ptr = NonNull::new(ctx.mmap.as_ptr() as *mut _)
                    .expect("mmap pointer should be non-null");
                msync(ptr, ctx.len, MsFlags::MS_INVALIDATE)?;
            }
        }

        Ok(())
    }
}
