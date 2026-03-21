use super::{FileContext, Op};
use crate::mmap;

/// 2 MiB per fadvise call stays under Linux's `max_sane_readahead()` cap.
const FADVISE_STEP: usize = 2 * 1024 * 1024;

/// How often to send TUI progress during the sequential page walk.
/// 512 pages = 2 MiB at 4K page size — fast enough for smooth display,
/// infrequent enough to avoid mincore overhead.
const PROGRESS_INTERVAL: usize = 512;

/// Readahead via fadvise, then walk every page with read_volatile.
pub struct Touch;

impl Op for Touch {
    type Output = ();

    fn execute(&self, ctx: &FileContext) -> crate::Result<()> {
        let mmap = &ctx.mmap;
        let len = ctx.len;

        if len == 0 {
            return Ok(());
        }

        let page_size = mmap::page_size();
        let total_pages = len.div_ceil(page_size);

        // Phase 1: kick off async readahead
        initiate_readahead(ctx);

        // Phase 2: walk every page to guarantee residency
        for page_idx in 0..total_pages {
            let offset = page_idx * page_size;
            // SAFETY: offset < len, within the mmap region.
            unsafe {
                std::ptr::read_volatile(mmap.as_ptr().add(offset));
            }

            if let Some(tx) = ctx.events
                && page_idx > 0
                && page_idx % PROGRESS_INTERVAL == 0
                && let Ok(residency) = mmap::mincore_residency(mmap, len)
            {
                let _ = tx.send(crate::events::Event::FileProgress {
                    path: ctx.path.display().to_string(),
                    residency,
                });
            }
        }

        // Final progress event with full residency
        if let Some(tx) = ctx.events
            && let Ok(residency) = mmap::mincore_residency(mmap, len)
        {
            let _ = tx.send(crate::events::Event::FileProgress {
                path: ctx.path.display().to_string(),
                residency,
            });
        }

        Ok(())
    }
}

fn initiate_readahead(ctx: &FileContext) {
    let len = ctx.len;

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsFd;
        let fd = ctx.file.as_fd();
        let offset = ctx.offset as i64;
        let len_i64 = len as i64;

        let _ = nix::fcntl::posix_fadvise(
            fd,
            offset,
            len_i64,
            nix::fcntl::PosixFadviseAdvice::POSIX_FADV_SEQUENTIAL,
        );
        for off in (0..len).step_by(FADVISE_STEP) {
            let chunk = (len - off).min(FADVISE_STEP) as i64;
            let _ = nix::fcntl::posix_fadvise(
                fd,
                offset + off as i64,
                chunk,
                nix::fcntl::PosixFadviseAdvice::POSIX_FADV_WILLNEED,
            );
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let step = FADVISE_STEP;
        for off in (0..len).step_by(step) {
            let chunk = (len - off).min(step);
            if let Err(e) = mmap::advise_willneed(&ctx.mmap, off, chunk) {
                tracing::warn!("madvise failed at offset {off}: {e}");
            }
        }
    }
}
