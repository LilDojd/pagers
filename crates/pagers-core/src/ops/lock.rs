use std::sync::Arc;

use memmap2::Mmap;

use super::touch::Touch;
use super::{FileContext, Op};
use crate::mmap;

/// Touch pages into cache, then lock them in physical memory with mlock(2).
pub struct Lock;

/// Holds the mmap alive after mlock — dropping this unmaps and unlocks.
///
/// Fields are private because they exist solely for RAII: the `Arc<Mmap>`
/// prevents munmap (which would release mlock), and the path is kept for
/// diagnostics if needed in the future.
pub struct LockedFile {
    #[allow(dead_code)]
    path: String,
    #[allow(dead_code)]
    mmap: Arc<Mmap>,
    #[allow(dead_code)]
    len: usize,
}

impl Op for Lock {
    type Output = LockedFile;

    fn execute(&self, ctx: &FileContext) -> crate::Result<LockedFile> {
        Touch.execute(ctx)?;
        mmap::mlock(&ctx.mmap, ctx.len)?;
        Ok(LockedFile {
            path: ctx.path.display().to_string(),
            mmap: Arc::clone(&ctx.mmap),
            len: ctx.len,
        })
    }
}
