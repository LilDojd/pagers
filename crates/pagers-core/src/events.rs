//! Events emitted during file processing for UI consumption.

use std::sync::Mutex;
use std::sync::mpsc::Sender;

use crate::mincore::DefaultPageMap;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Event<PM = DefaultPageMap> {
    /// A file has started processing. Includes initial residency snapshot.
    FileStart {
        path: String,
        total_pages: usize,
        residency: PM,
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
        residency: PM,
    },
    AllDone,
}

/// `Sync` wrapper around `Sender<Event>` for use with rayon.
pub struct EventSink<PM>(Mutex<Sender<Event<PM>>>);

impl<PM> EventSink<PM> {
    pub fn new(sender: Sender<Event<PM>>) -> Self {
        Self(Mutex::new(sender))
    }

    pub fn send(&self, event: Event<PM>) {
        let _ = self.0.lock().unwrap().send(event);
    }
}
