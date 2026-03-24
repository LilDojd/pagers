use memmap2::Advice;

use crate::mincore::PageMap;

use super::{FileContext, Op};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Touch;

impl Op for Touch {
    const LABEL: &str = "touched";
    const ACTION_SIGN: isize = 1;
    type Output = usize;

    fn action_pages(
        output: &usize,
        _total_pages: usize,
        _pages_in_core_before: Option<usize>,
        _pages_in_core_after: usize,
    ) -> usize {
        *output
    }

    fn execute<PM: PageMap + Sync>(&self, ctx: &FileContext<'_, PM>) -> crate::Result<usize> {
        let mmap = &ctx.mmap;
        let len = ctx.len;

        if len == 0 {
            return Ok(0);
        }

        let page_size = *crate::pagesize::PAGE_SIZE;
        let total_pages = len.div_ceil(page_size);

        let needs_touch = |i: &usize| ctx.residency.is_none_or(|r| !r.is_set(*i));

        let mut touched = 0usize;

        if (0..total_pages).any(|i| needs_touch(&i)) {
            const PROGRESS_INTERVAL: usize = 256;

            std::thread::scope(|s| {
                s.spawn(|| initiate_readahead(ctx));

                for page_idx in (0..total_pages).filter(needs_touch) {
                    let offset = page_idx * page_size;
                    unsafe {
                        std::ptr::read_volatile(mmap.as_ptr().add(offset));
                    }
                    touched += 1;
                    if let Some(on_progress) = &ctx.on_progress
                        && (page_idx + 1) % PROGRESS_INTERVAL == 0
                    {
                        on_progress(page_idx + 1, touched);
                    }
                }
            });
        }

        Ok(touched)
    }
}

fn initiate_readahead<PM: PageMap>(ctx: &FileContext<'_, PM>) {
    let offset = ctx.offset as libc::off_t;
    let len = ctx.len as libc::off_t;

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsFd;
        let fd = ctx.file.as_fd();
        let _ = nix::fcntl::posix_fadvise(
            fd,
            offset,
            len,
            nix::fcntl::PosixFadviseAdvice::POSIX_FADV_SEQUENTIAL,
        );
        let _ = nix::fcntl::posix_fadvise(
            fd,
            offset,
            len,
            nix::fcntl::PosixFadviseAdvice::POSIX_FADV_WILLNEED,
        );
    }

    let _ = ctx
        .mmap
        .advise_range(Advice::WillNeed, offset as usize, len as usize);
}
