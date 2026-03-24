use crate::mincore::PageMap;

use super::lock::{Lock, LockedFile};
use super::{FileContext, Op};
use crate::mlock;

/// [`Lock`] + `mlockall(MCL_CURRENT)` after all files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lockall;

impl Op for Lockall {
    const LABEL: &str = "locked";
    const ACTION_SIGN: isize = 1;
    type Output = LockedFile;

    fn action_pages(
        output: &LockedFile,
        _total_pages: usize,
        _pages_in_core_before: Option<usize>,
        _pages_in_core_after: usize,
    ) -> usize {
        output.pages_touched
    }

    fn execute<PM: PageMap + Sync>(&self, ctx: &FileContext<'_, PM>) -> crate::Result<LockedFile> {
        Lock.execute(ctx)
    }

    fn finish(&self) -> crate::Result<()> {
        mlock::mlockall_current()?;
        Ok(())
    }
}
