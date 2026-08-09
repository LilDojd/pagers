use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug)]
pub struct Cancellation<'a>(&'a AtomicBool);

impl<'a> Cancellation<'a> {
    pub const fn new(flag: &'a AtomicBool) -> Self {
        Self(flag)
    }

    pub fn is_cancelled(self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn check(self) -> crate::Result<()> {
        if self.is_cancelled() {
            Err(crate::Error::Cancelled)
        } else {
            Ok(())
        }
    }
}
