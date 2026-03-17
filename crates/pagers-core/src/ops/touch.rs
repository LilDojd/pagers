use std::time::Instant;

use rayon::prelude::*;

use super::{FileContext, Op};
use crate::mmap;

pub struct Touch {
    pub chunk_size: usize,
    pub timeout_secs: f64,
}

impl Op for Touch {
    type Output = ();

    fn execute(&self, ctx: &FileContext) -> anyhow::Result<()> {
        let mmap = &ctx.mmap;
        let len = ctx.len;
        let page_size = mmap::page_size();
        let total_pages = len.div_ceil(page_size);
        let timeout = std::time::Duration::from_secs_f64(self.timeout_secs);
        let start = Instant::now();

        let chunks: Vec<(usize, usize)> = (0..len)
            .step_by(self.chunk_size)
            .map(|off| {
                let chunk_len = (len - off).min(self.chunk_size);
                (off, chunk_len)
            })
            .collect();

        chunks.par_iter().for_each(|&(off, chunk_len)| {
            if let Err(e) = mmap::advise_willneed(mmap, off, chunk_len) {
                tracing::warn!("madvise failed at offset {off}: {e}");
            }
        });

        loop {
            let residency = mmap::mincore_residency(mmap, len)?;
            let resident_count = residency.iter().filter(|r| **r).count();

            if let Some(tx) = ctx.events {
                let _ = tx.send(crate::events::Event::FileProgress {
                    path: ctx.path.display().to_string(),
                    residency: residency.clone(),
                });
            }

            if resident_count == total_pages {
                break;
            }

            if start.elapsed() >= timeout {
                let non_resident: Vec<usize> = residency
                    .iter()
                    .enumerate()
                    .filter(|&(_, r)| !r)
                    .map(|(i, _)| i)
                    .collect();

                // Fault all pages in a background thread, poll mincore for progress
                let mmap_ptr = mmap.as_ptr() as usize;
                let non_resident_clone = non_resident.clone();
                let fault_handle = std::thread::spawn(move || {
                    non_resident_clone.par_iter().for_each(|&page_idx| {
                        let offset = page_idx * page_size;
                        if offset < len {
                            unsafe {
                                let _byte =
                                    std::ptr::read_volatile((mmap_ptr as *const u8).add(offset));
                            }
                        }
                    });
                });

                // Poll residency while faults are in progress
                let poll_interval = std::time::Duration::from_millis(100);
                while !fault_handle.is_finished() {
                    std::thread::sleep(poll_interval);
                    if let Some(tx) = ctx.events
                        && let Ok(r) = mmap::mincore_residency(mmap, len)
                    {
                        let _ = tx.send(crate::events::Event::FileProgress {
                            path: ctx.path.display().to_string(),
                            residency: r,
                        });
                    }
                }

                fault_handle.join().expect("fault thread panicked");

                // Final progress event
                if let Some(tx) = ctx.events
                    && let Ok(r) = mmap::mincore_residency(mmap, len)
                {
                    let _ = tx.send(crate::events::Event::FileProgress {
                        path: ctx.path.display().to_string(),
                        residency: r,
                    });
                }
                break;
            }
        }

        Ok(())
    }
}
