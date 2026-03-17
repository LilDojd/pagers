use memmap2::Mmap;
use std::time::Instant;

use crate::mmap;

use super::TouchParams;

/// Touch a file using chunked madvise(MADV_WILLNEED) with fallback.
pub(crate) fn touch_file(
    mmap: &Mmap,
    len: usize,
    params: TouchParams,
    path: &str,
    events: &Option<std::sync::mpsc::Sender<crate::events::Event>>,
) -> anyhow::Result<()> {
    let chunk_size = params.chunk_size;
    let page_size = mmap::page_size();
    let total_pages = len.div_ceil(page_size);
    let timeout = std::time::Duration::from_secs_f64(params.timeout_secs);
    let start = Instant::now();

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
            tracing::warn!("madvise failed at offset {off}: {e}");
        }
    });

    // Step 2: Poll residency until converged or timeout
    loop {
        let residency = mmap::mincore_residency(mmap, len)?;
        let resident_count = residency.iter().filter(|r| **r).count();

        if let Some(tx) = events {
            let _ = tx.send(crate::events::Event::FileProgress {
                path: path.to_string(),
                residency: residency.clone(),
            });
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
                        let _byte = std::ptr::read_volatile(mmap.as_ptr().add(offset));
                    }
                }
            });

            if let Some(tx) = events {
                let final_residency = mmap::mincore_residency(mmap, len)?;
                let _ = tx.send(crate::events::Event::FileProgress {
                    path: path.to_string(),
                    residency: final_residency,
                });
            }
            break;
        }
    }

    Ok(())
}
