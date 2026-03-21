use super::lock::{Lock, LockedFile};
use super::{FileContext, Op};
use crate::mmap;

/// [`Lock`] + `mlockall(MCL_CURRENT)` after all files.
pub struct Lockall;

impl Op for Lockall {
    const LABEL: &str = "locked";
    type Output = LockedFile;

    fn execute(&self, ctx: &FileContext) -> crate::Result<LockedFile> {
        Lock.execute(ctx)
    }

    fn finish(&self) -> crate::Result<()> {
        mmap::mlockall_current()?;
        Ok(())
    }
}
