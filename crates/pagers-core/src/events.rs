//! Events emitted during file processing for UI consumption.

use std::sync::Mutex;
use std::sync::mpsc::Sender;

use bitvec::prelude::*;

/// Events sent from core processing to UI consumers.
pub enum Event {
    /// A file has started processing. Includes initial residency snapshot.
    FileStart {
        path: String,
        total_pages: usize,
        residency: BitVec,
    },
    /// Residency update during touch/lock polling.
    FileProgress {
        path: String,
        pages_walked: usize,
    },
    /// File processing complete.
    FileDone {
        path: String,
        pages_in_core: usize,
        total_pages: usize,
        residency: BitVec,
    },
    AllDone,
}

/// `Sync` wrapper around `Sender<Event>` for use with rayon.
pub struct EventSink(Mutex<Sender<Event>>);

impl EventSink {
    pub fn new(sender: Sender<Event>) -> Self {
        Self(Mutex::new(sender))
    }

    pub fn send(&self, event: Event) {
        let _ = self.0.lock().unwrap().send(event);
    }
}
