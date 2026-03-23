use memmap2::Advice;

use super::{FileContext, Op};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Touch;

impl Op for Touch {
    const LABEL: &str = "touched";
    type Output = ();

    fn execute(&self, ctx: &FileContext) -> crate::Result<()> {
        let mmap = &ctx.mmap;
        let len = ctx.len;

        if len == 0 {
            return Ok(());
        }

        let page_size = *crate::pagesize::PAGE_SIZE;
        let total_pages = len.div_ceil(page_size);

        const PROGRESS_INTERVAL: usize = 256;

        std::thread::scope(|s| {
            s.spawn(|| initiate_readahead(ctx));

            for page_idx in 0..total_pages {
                let offset = page_idx * page_size;
                // SAFETY: offset < len, within the mmap region.
                unsafe {
                    std::ptr::read_volatile(mmap.as_ptr().add(offset));
                }
                if let Some(on_progress) = &ctx.on_progress
                    && (page_idx + 1) % PROGRESS_INTERVAL == 0
                {
                    on_progress(page_idx + 1);
                }
            }
        });

        Ok(())
    }
}

fn initiate_readahead(ctx: &FileContext) {
    let offset = ctx.offset as i64;
    let len = ctx.len as i64;

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
