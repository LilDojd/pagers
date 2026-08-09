//! Events emitted during file processing for UI consumption.

use std::sync::Arc;

use crate::mincore::DefaultPageMap;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Event<PM = DefaultPageMap> {
    /// A file has started processing. Includes initial residency snapshot.
    FileStart {
        id: usize,
        path: Arc<str>,
        total_pages: usize,
        residency: PM,
    },
    /// Residency update during touch/lock polling.
    FileProgress {
        id: usize,
        page_offset: usize,
        pages_walked: usize,
        resident: bool,
    },
    /// File processing complete.
    FileDone {
        id: usize,
    },
    AllDone,
}
