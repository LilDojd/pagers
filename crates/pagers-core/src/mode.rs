use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

use crate::events::{Event, EventSink};
use crate::mincore::{DefaultPageMap, PageMap};
use crate::ops::{
    self, FileContext, FileProcessed, FileRange, Op, PreparedFile, Stats, prepare_file,
};

pub trait DisplayMode<PM: PageMap = DefaultPageMap>: Sync {
    fn process_one<O: Op>(
        &self,
        op: &O,
        path: &Path,
        range: &FileRange,
        stats: &Stats,
    ) -> Option<O::Output>;

    fn finish(&self) {}
}

pub struct Tui<PM: PageMap = DefaultPageMap> {
    sink: EventSink<PM>,
}

impl<PM: PageMap> Tui<PM> {
    pub fn new(sender: Sender<Event<PM>>) -> Self {
        Self {
            sink: EventSink::new(sender),
        }
    }
}

impl<PM: PageMap + Clone + Send + Sync> DisplayMode<PM> for Tui<PM> {
    fn process_one<O: Op>(
        &self,
        op: &O,
        path: &Path,
        range: &FileRange,
        stats: &Stats,
    ) -> Option<O::Output> {
        let path_str = path.display().to_string();
        let full_file = FileRange {
            offset: 0,
            max_len: None,
        };

        let pf = match prepare_file(path, &full_file) {
            Ok(Some(pf)) => pf,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!("{}: {e}", path.display());
                return None;
            }
        };
        let residency: PM = match crate::mincore::residency(&pf.mmap, pf.len) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("{}: {e}", path.display());
                return None;
            }
        };
        let pages_in_core = residency.count_filled();
        let total_pages = pf.total_pages;

        stats.total_files.fetch_add(1, Ordering::Relaxed);
        stats.total_pages.fetch_add(total_pages, Ordering::Relaxed);
        stats
            .initial_pages_in_core
            .fetch_add(pages_in_core, Ordering::Relaxed);
        self.sink.send(Event::FileStart {
            path: path_str.clone(),
            total_pages,
            residency: residency.clone(),
        });

        let prepared_pf = if range.is_full_file() { Some(pf) } else { None };

        if O::SKIP_RESIDENCY {
            let result = match skip_process_file::<O, PM>(op, path, range, prepared_pf) {
                Ok(Some(r)) => r,
                Ok(None) => return None,
                Err(e) => {
                    tracing::warn!("{e}");
                    return None;
                }
            };

            let total_action = O::action_pages(
                result.output_ref(),
                result.total_pages(),
                result.pages_in_core_before(),
                result.pages_in_core_after(),
            );
            stats
                .action_pages
                .fetch_add(total_action, Ordering::Relaxed);

            self.sink.send(Event::FileProgress {
                path: path_str.clone(),
                page_offset: 0,
                pages_walked: total_pages,
                resident: O::ACTION_SIGN >= 0,
            });
            self.sink.send(Event::FileDone { path: path_str });

            return Some(result.into_output());
        }

        let page_offset = range.offset as usize / *crate::pagesize::PAGE_SIZE;
        let reported_action = std::sync::atomic::AtomicUsize::new(0);
        let on_progress = |pages_walked: usize, action_count: usize| {
            let action = action_count;
            let delta = action - reported_action.swap(action, Ordering::Relaxed);
            stats.action_pages.fetch_add(delta, Ordering::Relaxed);
            self.sink.send(Event::FileProgress {
                path: path_str.clone(),
                page_offset,
                pages_walked,
                resident: O::ACTION_SIGN >= 0,
            });
        };

        let prepared_full = prepared_pf.map(|pf| (pf, residency, pages_in_core));
        let result =
            match full_process_file::<O, PM>(op, path, range, Some(&on_progress), prepared_full) {
                Ok(Some(r)) => r,
                Ok(None) => return None,
                Err(e) => {
                    tracing::warn!("{e}");
                    return None;
                }
            };

        // Flush remaining action_pages not covered by the progress callback.
        let reported = reported_action.load(Ordering::Relaxed);
        let total_action = O::action_pages(
            result.output_ref(),
            result.total_pages(),
            result.pages_in_core_before(),
            result.pages_in_core_after(),
        );
        stats
            .action_pages
            .fetch_add(total_action - reported, Ordering::Relaxed);

        self.sink.send(Event::FileDone { path: path_str });

        Some(result.into_output())
    }

    fn finish(&self) {
        self.sink.send(Event::AllDone);
    }
}

pub struct Cli;

// Marker ZSTs for run-mode dispatch
pub struct TuiMode;
pub struct CliMode;
pub struct Daemon;
pub struct NoDaemon;

impl<PM: PageMap + Send + Sync> DisplayMode<PM> for Cli {
    fn process_one<O: Op>(
        &self,
        op: &O,
        path: &Path,
        range: &FileRange,
        stats: &Stats,
    ) -> Option<O::Output> {
        if O::SKIP_RESIDENCY {
            let result = match skip_process_file::<O, PM>(op, path, range, None) {
                Ok(Some(r)) => r,
                Ok(None) => return None,
                Err(e) => {
                    tracing::warn!("{e}");
                    return None;
                }
            };
            cli_record_stats::<O>(&result, stats);
            tracing::info!("{}: {} pages", path.display(), result.total_pages());
            return Some(result.into_output());
        }

        let result = match counts_process_file::<O, PM>(op, path, range) {
            Ok(Some(r)) => r,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!("{e}");
                return None;
            }
        };
        cli_record_stats::<O>(&result, stats);
        tracing::info!(
            "{}: {}/{} pages resident",
            path.display(),
            result.pages_in_core_after(),
            result.total_pages(),
        );
        Some(result.into_output())
    }
}

pub(crate) fn full_process_file<O: Op, PM: PageMap + Sync>(
    op: &O,
    path: &Path,
    range: &FileRange,
    on_progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    prepared: Option<(PreparedFile, PM, usize)>,
) -> crate::Result<Option<ops::FullResult<O::Output, PM>>> {
    let (pf, residency_before, pages_in_core_before) = match prepared {
        Some(tuple) => tuple,
        None => {
            let Some(pf) = prepare_file(path, range)? else {
                return Ok(None);
            };
            let residency_before: PM = crate::mincore::residency(&pf.mmap, pf.len)?;
            let pages_in_core_before = residency_before.count_filled();
            (pf, residency_before, pages_in_core_before)
        }
    };

    let ctx = FileContext {
        file: &pf.file,
        path,
        mmap: std::sync::Arc::clone(&pf.mmap),
        offset: pf.offset,
        len: pf.len,
        on_progress,
        residency: Some(&residency_before),
    };

    let output = op.execute(&ctx)?;

    let (pages_in_core_after, residency_before, residency_after) = if O::MUTATES_RESIDENCY {
        let fill = O::ACTION_SIGN >= 0;
        let after = PM::from_bools(std::iter::repeat_n(fill, pf.total_pages));
        let count = if fill { pf.total_pages } else { 0 };
        (count, Some(residency_before), Some(after))
    } else {
        (pages_in_core_before, None, Some(residency_before))
    };

    Ok(Some(ops::FullResult {
        output,
        total_pages: pf.total_pages,
        pages_in_core_before,
        pages_in_core_after,
        residency_before,
        residency_after,
    }))
}

pub(crate) fn counts_process_file<O: Op, PM: PageMap + Sync>(
    op: &O,
    path: &Path,
    range: &FileRange,
) -> crate::Result<Option<ops::CountsResult<O::Output>>> {
    let Some(pf) = prepare_file(path, range)? else {
        return Ok(None);
    };

    let residency: Option<PM> = if O::MUTATES_RESIDENCY {
        Some(crate::mincore::residency(&pf.mmap, pf.len)?)
    } else {
        None
    };

    let ctx = FileContext {
        file: &pf.file,
        path,
        mmap: std::sync::Arc::clone(&pf.mmap),
        offset: pf.offset,
        len: pf.len,
        on_progress: None,
        residency: residency.as_ref(),
    };

    let output = op.execute(&ctx)?;

    let pages_in_core_after = if O::MUTATES_RESIDENCY {
        if O::ACTION_SIGN >= 0 {
            pf.total_pages
        } else {
            0
        }
    } else {
        counts_page_count::<PM>(&pf.file, &pf.mmap, pf.offset, pf.len)?
    };

    Ok(Some(ops::CountsResult {
        output,
        total_pages: pf.total_pages,
        pages_in_core_after,
    }))
}

pub(crate) fn skip_process_file<O: Op, PM: PageMap + Sync>(
    op: &O,
    path: &Path,
    range: &FileRange,
    prepared: Option<PreparedFile>,
) -> crate::Result<Option<ops::SkipResult<O::Output>>> {
    let pf = match prepared {
        Some(pf) => pf,
        None => {
            let Some(pf) = prepare_file(path, range)? else {
                return Ok(None);
            };
            pf
        }
    };

    let ctx = FileContext {
        file: &pf.file,
        path,
        mmap: std::sync::Arc::clone(&pf.mmap),
        offset: pf.offset,
        len: pf.len,
        on_progress: None,
        residency: None::<&PM>,
    };

    let output = op.execute(&ctx)?;

    Ok(Some(ops::SkipResult {
        output,
        total_pages: pf.total_pages,
    }))
}

fn cli_record_stats<O: Op>(result: &impl FileProcessed<Output = O::Output>, stats: &Stats) {
    let action = O::action_pages(
        result.output_ref(),
        result.total_pages(),
        result.pages_in_core_before(),
        result.pages_in_core_after(),
    );
    let signed_action = action as isize * O::ACTION_SIGN;
    let initial = (result.pages_in_core_after() as isize - signed_action).max(0) as usize;
    stats
        .total_pages
        .fetch_add(result.total_pages(), Ordering::Relaxed);
    stats
        .initial_pages_in_core
        .fetch_add(initial, Ordering::Relaxed);
    stats.action_pages.fetch_add(action, Ordering::Relaxed);
    stats.total_files.fetch_add(1, Ordering::Relaxed);
}

#[allow(unused_variables)]
fn counts_page_count<PM: PageMap>(
    file: &std::fs::File,
    mmap: &memmap2::Mmap,
    offset: u64,
    len: usize,
) -> crate::Result<usize> {
    #[cfg(target_os = "linux")]
    if *crate::cachestat::SUPPORTED {
        use std::os::unix::io::AsFd;
        return Ok(crate::cachestat::cached_pages(file.as_fd(), offset, len as u64)?.try_into()?);
    }
    let residency: PM = crate::mincore::residency(mmap, len)?;
    Ok(residency.count_filled())
}
