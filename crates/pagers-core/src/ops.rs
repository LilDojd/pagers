//! Per-file operations: touch, query, evict, lock.

use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};
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
    pub timeout_secs: u64,
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
    pub verbose: u8,
    pub quiet: bool,
    pub offset: u64,
    pub max_len: Option<u64>,
}

/// Result of processing a single file — holds the mmap if locked.
pub struct LockedFile {
    pub _path: String,
    pub _mmap: Mmap,
    pub _len: usize,
}

/// Create a progress bar for a file being touched.
pub fn make_progress_bar(path: &str, quiet: bool) -> Option<ProgressBar> {
    if quiet {
        return None;
    }
    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} pages ({percent}%) {msg}"
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    pb.set_message(path.to_string());
    Some(pb)
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

    stats.total_pages.fetch_add(pages_in_range as i64, Ordering::Relaxed);
    stats.total_files.fetch_add(1, Ordering::Relaxed);

    if matches!(config.operation, Operation::Evict) {
        evict_file(&file, path, offset as i64, len_of_range as i64, config)?;
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
    stats.total_pages_in_core.fetch_add(pages_in_core, Ordering::Relaxed);

    let touch_params = match config.operation {
        Operation::Touch(p) | Operation::Lock(p) => Some(p),
        _ => None,
    };

    if let Some(params) = touch_params {
        let pb = make_progress_bar(&path.display().to_string(), config.quiet);
        touch_file(&mmap, len_of_range, params, pb.as_ref())?;

        let residency_after = mmap::mincore_residency(&mmap, len_of_range)?;
        let new_in_core: i64 = residency_after.iter().filter(|r| **r).count() as i64;
        let delta = new_in_core - pages_in_core;
        stats.total_pages_in_core.fetch_add(delta, Ordering::Relaxed);
    }

    if matches!(config.operation, Operation::Lock(_)) {
        mmap::mlock(&mmap, len_of_range)?;
        return Ok(Some(LockedFile {
            _path: path.display().to_string(),
            _mmap: mmap,
            _len: len_of_range,
        }));
    }

    Ok(None)
}

/// Touch a file using chunked madvise(MADV_WILLNEED) with fallback.
fn touch_file(
    mmap: &Mmap,
    len: usize,
    params: TouchParams,
    progress: Option<&ProgressBar>,
) -> anyhow::Result<()> {
    let chunk_size = params.chunk_size;
    let page_size = mmap::page_size();
    let total_pages = len.div_ceil(page_size);
    let timeout = std::time::Duration::from_secs(params.timeout_secs);
    let start = Instant::now();

    if let Some(pb) = progress {
        pb.set_length(total_pages as u64);
    }

    // Step 1: Issue madvise(MADV_WILLNEED) on all chunks
    let chunks: Vec<(usize, usize)> = (0..len)
        .step_by(chunk_size)
        .map(|off| {
            let chunk_len = (len - off).min(chunk_size);
            (off, chunk_len)
        })
        .collect();

    use rayon::prelude::*;
    chunks.par_iter().for_each(|&(off, chunk_len)| {
        if let Err(e) = mmap::advise_willneed(mmap, off, chunk_len) {
            eprintln!("pagers: WARNING: madvise failed at offset {off}: {e}");
        }
    });

    // Step 2: Poll residency until converged or timeout
    loop {
        let residency = mmap::mincore_residency(mmap, len)?;
        let resident_count = residency.iter().filter(|r| **r).count();

        if let Some(pb) = progress {
            pb.set_position(resident_count as u64);
        }

        if resident_count == total_pages {
            break;
        }

        if start.elapsed() >= timeout {
            // Fallback: parallel manual touch for non-resident pages
            let non_resident: Vec<usize> = residency
                .iter()
                .enumerate()
                .filter(|&(_, r)| !r)
                .map(|(i, _)| i)
                .collect();

            non_resident.par_iter().for_each(|&page_idx| {
                let offset = page_idx * page_size;
                if offset < len {
                    unsafe {
                        let _byte = std::ptr::read_volatile(
                            mmap.as_ptr().add(offset),
                        );
                    }
                }
            });

            if let Some(pb) = progress {
                pb.set_position(total_pages as u64);
            }
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if let Some(pb) = progress {
        pb.finish();
    }

    Ok(())
}

/// Evict pages from cache.
fn evict_file(file: &File, path: &Path, offset: i64, len: i64, config: &OpConfig) -> anyhow::Result<()> {
    if config.verbose > 0 {
        eprintln!("Evicting {}", path.display());
    }

    #[cfg(target_os = "linux")]
    {
        mmap::evict(file.as_raw_fd(), offset, len)?;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = file.as_raw_fd(); // suppress unused warning
        // On macOS, mmap + msync(MS_INVALIDATE)
        let mmap = unsafe {
            MmapOptions::new()
                .offset(offset as u64)
                .len(len as usize)
                .map(file)?
        };
        unsafe {
            if libc::msync(
                mmap.as_ptr() as *mut libc::c_void,
                len as usize,
                libc::MS_INVALIDATE,
            ) != 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }

    Ok(())
}
