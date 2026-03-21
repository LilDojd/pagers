use super::{FileContext, Op};

pub struct Evict;

impl Op for Evict {
    type Output = ();

    fn execute(&self, ctx: &FileContext) -> crate::Result<()> {
        tracing::debug!("Evicting {}", ctx.path.display());

        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsFd;
            nix::fcntl::posix_fadvise(
                ctx.file.as_fd(),
                ctx.offset as i64,
                ctx.len as i64,
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
