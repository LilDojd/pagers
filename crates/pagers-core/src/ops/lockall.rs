use crate::mincore::PageMap;

use super::lock::{Lock, LockedFile};
use super::{FileContext, Op, ResidencyEffect};
use crate::mlock;

/// [`Lock`] + `mlockall(MCL_CURRENT)` after all files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lockall;

impl Op for Lockall {
    const LABEL: &str = "locked";
    const EFFECT: ResidencyEffect = ResidencyEffect::Populate;
    type Output = LockedFile;

    fn execute<PM: PageMap + Sync>(&self, ctx: &FileContext<'_, PM>) -> crate::Result<LockedFile> {
        Lock.execute(ctx)
    }

    fn finish(&self) -> crate::Result<()> {
        mlock::mlockall_current()?;
        Ok(())
    }
}
