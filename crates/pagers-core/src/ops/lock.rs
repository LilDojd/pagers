use std::sync::Arc;

use memmap2::Mmap;

use super::touch::Touch;
use super::{FileContext, Op};
use crate::mmap;

pub struct Lock {
    pub touch: Touch,
}

/// Holds the mmap alive after mlock -- drop unmaps and unlocks.
pub struct LockedFile {
    pub _path: String,
    pub _mmap: Arc<Mmap>,
    pub _len: usize,
}

impl Op for Lock {
    type Output = LockedFile;

    fn execute(&self, ctx: &FileContext) -> crate::Result<LockedFile> {
        self.touch.execute(ctx)?;
        mmap::mlock(&ctx.mmap, ctx.len)?;
        Ok(LockedFile {
            _path: ctx.path.display().to_string(),
            _mmap: Arc::clone(&ctx.mmap),
            _len: ctx.len,
        })
    }
}
