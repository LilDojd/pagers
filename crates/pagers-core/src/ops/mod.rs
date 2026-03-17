//! Per-file operations: touch, query, evict, lock.

mod evict;
mod touch;

use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};

use memmap2::{Mmap, MmapOptions};

use crate::mmap;

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

#[derive(Clone, Copy)]
pub struct TouchParams {
    pub chunk_size: usize,
    pub timeout_secs: f64,
}

#[derive(Clone, Copy)]
pub enum Operation {
    Query,
    Touch(TouchParams),
    Evict,
    Lock(TouchParams),
}

pub struct OpConfig {
    pub operation: Operation,
    pub offset: u64,
    pub max_len: Option<u64>,
    pub events: Option<std::sync::mpsc::Sender<crate::events::Event>>,
}

/// Result of processing a single file — holds the mmap if locked.
pub struct LockedFile {
    pub _path: String,
    pub _mmap: Mmap,
    pub _len: usize,
}

/// Process a single file: touch, query, evict, or lock.
/// Returns a LockedFile if lock mode is active (caller must keep it alive).
pub fn process_file(
    path: &Path,
    config: &OpConfig,
    stats: &Stats,
) -> anyhow::Result<Option<LockedFile>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let file_len = metadata.len();

    if file_len == 0 {
        return Ok(None);
    }

    // Calculate the range to operate on
    let offset = config.offset;
    if offset >= file_len {
        anyhow::bail!("file {} smaller than offset", path.display());
    }

    let len_of_range = match config.max_len {
        Some(max) if (offset + max) < file_len => max as usize,
        _ => (file_len - offset) as usize,
    };

    let page_size = mmap::page_size();
    let pages_in_range = len_of_range.div_ceil(page_size);

    stats
        .total_pages
        .fetch_add(pages_in_range as i64, Ordering::Relaxed);
    stats.total_files.fetch_add(1, Ordering::Relaxed);

    if matches!(config.operation, Operation::Evict) {
        evict::evict_file(&file, path, offset as i64, len_of_range as i64)?;
        return Ok(None);
    }

    let mmap = unsafe {
        MmapOptions::new()
            .offset(offset)
            .len(len_of_range)
            .map(&file)?
    };

    let residency = mmap::mincore_residency(&mmap, len_of_range)?;
    let pages_in_core: i64 = residency.iter().filter(|r| **r).count() as i64;
    stats
        .total_pages_in_core
        .fetch_add(pages_in_core, Ordering::Relaxed);

    if let Some(tx) = &config.events {
        let _ = tx.send(crate::events::Event::FileStart {
            path: path.display().to_string(),
            total_pages: pages_in_range,
            residency: residency.clone(),
        });
    }

    let touch_params = match config.operation {
        Operation::Touch(p) | Operation::Lock(p) => Some(p),
        _ => None,
    };

    if let Some(params) = touch_params {
        touch::touch_file(
            &mmap,
            len_of_range,
            params,
            &path.display().to_string(),
            &config.events,
        )?;

        let residency_after = mmap::mincore_residency(&mmap, len_of_range)?;
        let new_in_core: i64 = residency_after.iter().filter(|r| **r).count() as i64;
        let delta = new_in_core - pages_in_core;
        stats
            .total_pages_in_core
            .fetch_add(delta, Ordering::Relaxed);
    }

    if matches!(config.operation, Operation::Lock(_)) {
        mmap::mlock(&mmap, len_of_range)?;
        if let Some(tx) = &config.events {
            let final_residency = mmap::mincore_residency(&mmap, len_of_range)?;
            let final_in_core = final_residency.iter().filter(|r| **r).count();
            let _ = tx.send(crate::events::Event::FileDone {
                path: path.display().to_string(),
                pages_in_core: final_in_core,
                total_pages: pages_in_range,
            });
        }
        return Ok(Some(LockedFile {
            _path: path.display().to_string(),
            _mmap: mmap,
            _len: len_of_range,
        }));
    }

    if let Some(tx) = &config.events {
        let final_residency = mmap::mincore_residency(&mmap, len_of_range)?;
        let final_in_core = final_residency.iter().filter(|r| **r).count();
        let _ = tx.send(crate::events::Event::FileDone {
            path: path.display().to_string(),
            pages_in_core: final_in_core,
            total_pages: pages_in_range,
        });
    }

    Ok(None)
}
