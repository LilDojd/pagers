use super::lock::{Lock, LockedFile};
use super::{FileContext, Op};
use crate::mmap;

/// [`Lock`] + `mlockall(MCL_CURRENT)` after all files.
pub struct Lockall {
    pub lock: Lock,
}

impl Op for Lockall {
    type Output = LockedFile;

    fn execute(&self, ctx: &FileContext) -> crate::Result<LockedFile> {
        self.lock.execute(ctx)
    }

    fn finish(&self) -> crate::Result<()> {
        mmap::mlockall_current()?;
        Ok(())
    }
}
