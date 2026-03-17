use std::time::Instant;

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

        use rayon::prelude::*;
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

                non_resident.par_iter().for_each(|&page_idx| {
                    let offset = page_idx * page_size;
                    if offset < len {
                        unsafe {
                            let _byte = std::ptr::read_volatile(mmap.as_ptr().add(offset));
                        }
                    }
                });

                if let Some(tx) = ctx.events {
                    let final_residency = mmap::mincore_residency(mmap, len)?;
                    let _ = tx.send(crate::events::Event::FileProgress {
                        path: ctx.path.display().to_string(),
                        residency: final_residency,
                    });
                }
                break;
            }
        }

        Ok(())
    }
}
