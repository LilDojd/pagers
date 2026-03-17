//! Events emitted during file processing for UI consumption.

/// Events sent from core processing to UI consumers.
pub enum Event {
    /// A file has started processing. Includes initial residency snapshot.
    FileStart {
        path: String,
        total_pages: usize,
        residency: Vec<bool>,
    },
    /// Residency update during touch/lock polling.
    FileProgress {
        path: String,
        residency: Vec<bool>,
    },
    /// File processing complete.
    FileDone {
        path: String,
        pages_in_core: usize,
        total_pages: usize,
    },
}
