//! Per-file operations: touch, query, evict, lock.

mod evict;
mod lock;
mod query;
mod touch;

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::Sender;

use memmap2::{Mmap, MmapOptions};

use crate::events::Event;
use crate::mmap;

pub use evict::Evict;
pub use lock::{Lock, LockedFile};
pub use query::Query;
pub use touch::Touch;

/// Trait for file-level page cache operations.
pub trait Op: Sync {
    type Output: Send;
    fn execute(&self, ctx: &FileContext) -> anyhow::Result<Self::Output>;
}

/// Context prepared by the framework for each file.
pub struct FileContext<'a> {
    pub file: &'a File,
    pub path: &'a Path,
    pub mmap: Arc<Mmap>,
    pub offset: u64,
    pub len: usize,
    pub events: Option<&'a Sender<Event>>,
}

/// Byte range within a file to operate on.
#[derive(Clone, Copy)]
pub struct FileRange {
    pub offset: u64,
    pub max_len: Option<u64>,
}

/// Accumulated statistics across all files processed.
pub struct Stats {
    pub total_pages: AtomicI64,
    pub total_pages_in_core: AtomicI64,
    pub total_files: AtomicI64,
    pub total_dirs: AtomicI64,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            total_pages: AtomicI64::new(0),
            total_pages_in_core: AtomicI64::new(0),
            total_files: AtomicI64::new(0),
            total_dirs: AtomicI64::new(0),
        }
    }
}

/// Process a single file with the given operation.
/// Returns `None` for empty files, `Some(output)` otherwise.
pub fn process_file<O: Op>(
    op: &O,
    path: &Path,
    range: &FileRange,
    stats: &Stats,
    events: Option<&Sender<Event>>,
) -> anyhow::Result<Option<O::Output>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let file_len = metadata.len();

    if file_len == 0 {
        return Ok(None);
    }

    let offset = range.offset;
    if offset >= file_len {
        anyhow::bail!("file {} smaller than offset", path.display());
    }

    let len_of_range = match range.max_len {
        Some(max) if (offset + max) < file_len => max as usize,
        _ => (file_len - offset) as usize,
    };

    let page_size = mmap::page_size();
    let pages_in_range = len_of_range.div_ceil(page_size);

    stats
        .total_pages
        .fetch_add(pages_in_range as i64, Ordering::Relaxed);
    stats.total_files.fetch_add(1, Ordering::Relaxed);

    let mmap = Arc::new(unsafe {
        MmapOptions::new()
            .offset(offset)
            .len(len_of_range)
            .map(&file)?
    });

    let residency = mmap::mincore_residency(&mmap, len_of_range)?;
    let pages_in_core: i64 = residency.iter().filter(|r| **r).count() as i64;
    stats
        .total_pages_in_core
        .fetch_add(pages_in_core, Ordering::Relaxed);

    if let Some(tx) = events {
        let _ = tx.send(Event::FileStart {
            path: path.display().to_string(),
            total_pages: pages_in_range,
            residency,
        });
    }

    let ctx = FileContext {
        file: &file,
        path,
        mmap: Arc::clone(&mmap),
        offset,
        len: len_of_range,
        events,
    };

    let output = op.execute(&ctx)?;

    let final_residency = mmap::mincore_residency(&mmap, len_of_range)?;
    let final_in_core: i64 = final_residency.iter().filter(|r| **r).count() as i64;
    let delta = final_in_core - pages_in_core;
    stats
        .total_pages_in_core
        .fetch_add(delta, Ordering::Relaxed);

    if let Some(tx) = events {
        let _ = tx.send(Event::FileDone {
            path: path.display().to_string(),
            pages_in_core: final_in_core as usize,
            total_pages: pages_in_range,
        });
    }

    Ok(Some(output))
}
