use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Debug)]
pub struct Cancellation(Arc<State>);

#[derive(Debug)]
struct State {
    cancelled: AtomicBool,
    event: Condvar,
    lock: Mutex<()>,
}

impl Cancellation {
    pub fn new() -> Self {
        Self(Arc::new(State {
            cancelled: AtomicBool::new(false),
            event: Condvar::new(),
            lock: Mutex::new(()),
        }))
    }

    pub fn cancel(&self) {
        let _guard = self.0.lock.lock().unwrap();
        if !self.0.cancelled.swap(true, Ordering::Release) {
            self.0.event.notify_all();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    pub fn wait(&self) {
        let guard = self.0.lock.lock().unwrap();
        drop(
            self.0
                .event
                .wait_while(guard, |_| !self.is_cancelled())
                .unwrap(),
        );
    }

    pub fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            Err(crate::Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}
