//! Per-file operations: touch, query, evict, lock.

mod evict;
mod lock;
mod lockall;
mod query;
pub(crate) mod touch;

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::Sender;

use memmap2::{Mmap, MmapOptions};

use crate::Error;
use crate::events::Event;
use crate::mmap;

pub use evict::Evict;
pub use lock::{Lock, LockedFile};
pub use lockall::Lockall;
pub use query::Query;
pub use touch::Touch;

/// Trait for file-level page cache operations.
pub trait Op: Sync {
    type Output: Send;
    fn execute(&self, ctx: &FileContext) -> crate::Result<Self::Output>;

    fn finish(&self) -> crate::Result<()> {
        Ok(())
    }
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

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
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

pub fn send_file_start(
    path: &Path,
    range: &FileRange,
    tx: &Sender<Event>,
) -> crate::Result<()> {
    let file =
        File::open(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    let file_len = file
        .metadata()
        .map_err(|e| Error::io(path.display().to_string(), e))?
        .len();
    if file_len == 0 {
        return Ok(());
    }
    let offset = range.offset;
    if offset >= file_len {
        return Ok(());
    }
    let len_of_range = match range.max_len {
        Some(max) if (offset + max) < file_len => max as usize,
        _ => (file_len - offset) as usize,
    };
    let pages_in_range = len_of_range.div_ceil(mmap::page_size());
    let mmap = unsafe {
        MmapOptions::new()
            .offset(offset)
            .len(len_of_range)
            .map(&file)
            .map_err(|e| Error::io(path.display().to_string(), e))?
    };
    let residency = mmap::mincore_residency(&mmap, len_of_range)?;
    let _ = tx.send(Event::FileStart {
        path: path.display().to_string(),
        total_pages: pages_in_range,
        residency,
    });
    Ok(())
}

/// Cached page count via cachestat(2).
#[cfg(target_os = "linux")]
fn cachestat_count(file: &File, offset: u64, len: u64) -> crate::Result<i64> {
    use std::os::unix::io::AsFd;
    Ok(crate::cachestat::cached_pages(file.as_fd(), offset, len)? as i64)
}
/// Process a single file with the given operation.
/// Returns `None` for empty files, `Some(output)` otherwise.
pub fn process_file<O: Op>(
    op: &O,
    path: &Path,
    range: &FileRange,
    stats: &Stats,
    events: Option<&Sender<Event>>,
    discovered: bool,
) -> crate::Result<Option<O::Output>> {
    let file =
        File::open(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    let metadata = file
        .metadata()
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    let file_len = metadata.len();

    if file_len == 0 {
        return Ok(None);
    }

    let offset = range.offset;
    if offset >= file_len {
        return Err(Error::OffsetBeyondFile {
            path: path.to_path_buf(),
            offset,
            file_len,
        });
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
            .map(&file)
            .map_err(|e| Error::io(path.display().to_string(), e))?
    });

    #[cfg(target_os = "linux")]
    let use_cachestat = events.is_none() && crate::cachestat::supported();
    #[cfg(not(target_os = "linux"))]
    let use_cachestat = false;

    let pages_in_core: i64;
    if use_cachestat {
        pages_in_core = cachestat_count(&file, offset, len_of_range as u64)?;
    } else {
        let residency = mmap::mincore_residency(&mmap, len_of_range)?;
        pages_in_core = residency.iter().filter(|r| **r).count() as i64;

        if let Some(tx) = events
            && !discovered
        {
            let _ = tx.send(Event::FileStart {
                path: path.display().to_string(),
                total_pages: pages_in_range,
                residency,
            });
        }
    }

    stats
        .total_pages_in_core
        .fetch_add(pages_in_core, Ordering::Relaxed);

    let ctx = FileContext {
        file: &file,
        path,
        mmap: Arc::clone(&mmap),
        offset,
        len: len_of_range,
        events,
    };

    let output = op.execute(&ctx)?;

    let final_in_core: i64;
    if use_cachestat {
        final_in_core = cachestat_count(&file, offset, len_of_range as u64)?;
    } else {
        let final_residency = mmap::mincore_residency(&mmap, len_of_range)?;
        final_in_core = final_residency.iter().filter(|r| **r).count() as i64;

        if let Some(tx) = events {
            let _ = tx.send(Event::FileDone {
                path: path.display().to_string(),
                pages_in_core: final_in_core as usize,
                total_pages: pages_in_range,
                residency: final_residency,
            });
        }
    }

    let delta = final_in_core - pages_in_core;
    stats
        .total_pages_in_core
        .fetch_add(delta, Ordering::Relaxed);

    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_file(pages: usize) -> (tempfile::NamedTempFile, usize) {
        let page_size = mmap::page_size();
        let size = page_size * pages;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&vec![0xABu8; size]).unwrap();
        f.flush().unwrap();
        (f, size)
    }

    #[test]
    fn test_process_file_query_counts_pages() {
        let (f, _size) = create_temp_file(4);
        let stats = Stats::new();
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(&Query, f.path(), &range, &stats, None, false).unwrap();
        assert!(result.is_some());
        assert_eq!(stats.total_pages.load(Ordering::Relaxed), 4);
        assert_eq!(stats.total_files.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_process_file_empty_file_returns_none() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let stats = Stats::new();
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(&Query, f.path(), &range, &stats, None, false).unwrap();
        assert!(result.is_none());
        assert_eq!(stats.total_files.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_process_file_offset_beyond_file() {
        let (f, _) = create_temp_file(1);
        let stats = Stats::new();
        let range = FileRange {
            offset: 1_000_000,
            max_len: None,
        };
        let result = process_file(&Query, f.path(), &range, &stats, None, false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, Error::OffsetBeyondFile { .. }),
            "expected OffsetBeyondFile, got: {err}"
        );
    }

    #[test]
    fn test_process_file_with_max_len() {
        let (f, _) = create_temp_file(8);
        let page_size = mmap::page_size();
        let stats = Stats::new();
        let range = FileRange {
            offset: 0,
            max_len: Some((page_size * 2) as u64),
        };
        process_file(&Query, f.path(), &range, &stats, None, false).unwrap();
        assert_eq!(stats.total_pages.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_process_file_touch_makes_resident() {
        let (f, size) = create_temp_file(4);
        let stats = Stats::new();
        let range = FileRange {
            offset: 0,
            max_len: None,
        };

        process_file(&Evict, f.path(), &range, &stats, None, false).unwrap();

        let stats2 = Stats::new();
        process_file(&Touch, f.path(), &range, &stats2, None, false).unwrap();

        let file = File::open(f.path()).unwrap();
        let mmap_check = unsafe {
            memmap2::MmapOptions::new().len(size).map(&file).unwrap()
        };
        let residency = mmap::mincore_residency(&mmap_check, size).unwrap();
        assert!(
            residency.iter().all(|&r| r),
            "expected all pages resident after touch"
        );
    }

    #[test]
    fn test_process_file_evict_succeeds() {
        let (f, _) = create_temp_file(4);
        let stats = Stats::new();
        let range = FileRange {
            offset: 0,
            max_len: None,
        };

        let result = process_file(&Evict, f.path(), &range, &stats, None, false);
        assert!(result.is_ok());
        assert_eq!(stats.total_files.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_process_file_sends_events() {
        let (f, _) = create_temp_file(4);
        let stats = Stats::new();
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        process_file(&Query, f.path(), &range, &stats, Some(&tx), false).unwrap();
        drop(tx);

        let events: Vec<_> = rx.iter().collect();
        assert!(events.len() >= 2, "expected at least 2 events, got {}", events.len());
        assert!(matches!(&events[0], crate::events::Event::FileStart { .. }));
        assert!(matches!(
            events.last().unwrap(),
            crate::events::Event::FileDone { .. }
        ));
    }

    #[test]
    fn test_process_file_nonexistent_returns_error() {
        let stats = Stats::new();
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(
            &Query,
            std::path::Path::new("/nonexistent/file.dat"),
            &range,
            &stats,
            None,
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_stats_default() {
        let stats = Stats::default();
        assert_eq!(stats.total_pages.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_pages_in_core.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_files.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_dirs.load(Ordering::Relaxed), 0);
    }
}
