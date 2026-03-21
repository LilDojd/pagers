use std::sync::Arc;

use memmap2::Mmap;

use super::touch::Touch;
use super::{FileContext, Op};
use crate::mmap;

/// Touch pages into cache, then lock them in physical memory with mlock(2).
pub struct Lock;

/// Holds the mmap alive after mlock — dropping this unmaps and unlocks.
pub struct LockedFile {
    _mmap: Arc<Mmap>,
}

impl Op for Lock {
    const LABEL: &str = "locked";
    type Output = LockedFile;

    fn execute(&self, ctx: &FileContext) -> crate::Result<LockedFile> {
        Touch.execute(ctx)?;
        mmap::mlock(&ctx.mmap, ctx.len)?;
        Ok(LockedFile {
            _mmap: Arc::clone(&ctx.mmap),
        })
    }
}
