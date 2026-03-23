use super::{FileContext, Op};
use crate::mmap;

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

        let page_size = mmap::page_size();
        let total_pages = len.div_ceil(page_size);

        std::thread::scope(|s| {
            s.spawn(|| initiate_readahead(ctx));

            for page_idx in 0..total_pages {
                let offset = page_idx * page_size;
                // SAFETY: offset < len, within the mmap region.
                unsafe {
                    std::ptr::read_volatile(mmap.as_ptr().add(offset));
                }

                if let Some(sink) = ctx.events
                    && page_idx > 0
                    && page_idx % 4096 == 0
                {
                    sink.send(crate::events::Event::FileProgress {
                        path: ctx.path.display().to_string(),
                        pages_walked: page_idx,
                    });
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

    let _ = mmap::advise_willneed(&ctx.mmap, ctx.offset as usize, ctx.len);
}
