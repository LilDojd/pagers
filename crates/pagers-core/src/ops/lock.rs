use std::sync::Arc;

use memmap2::Mmap;

use crate::mincore::PageMap;

use super::touch::Touch;
use super::{FileContext, Op, ResidencyEffect};
use crate::mlock;

/// Touch pages into cache, then lock them in physical memory with mlock(2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lock;

/// Holds the mmap alive after mlock — dropping this unmaps and unlocks.
#[derive(Debug)]
pub struct LockedFile {
    _mmap: Arc<Mmap>,
    pub pages_touched: usize,
}

impl Op for Lock {
    const LABEL: &str = "locked";
    const EFFECT: ResidencyEffect = ResidencyEffect::Populate;
    type Output = LockedFile;

    fn execute<PM: PageMap + Sync>(&self, ctx: &FileContext<'_, PM>) -> crate::Result<LockedFile> {
        let pages_touched = Touch.execute(ctx)?;
        mlock::mlock(ctx.mmap(), ctx.len())?;
        Ok(LockedFile {
            _mmap: ctx.mapping(),
            pages_touched,
        })
    }
}
