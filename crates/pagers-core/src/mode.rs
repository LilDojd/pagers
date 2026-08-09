use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

use crate::Cancellation;
use crate::events::{Event, EventSink};
use crate::mincore::{DefaultPageMap, PageMap};
use crate::ops::{
    self, FileContext, FileProcessed, FileRange, Op, PreparedFile, ResidencyEffect, Stats,
    prepare_file,
};

pub trait DisplayMode<PM: PageMap = DefaultPageMap>: Sync {
    fn process_one<O: Op>(
        &self,
        op: &O,
        path: &Path,
        range: &FileRange,
        stats: &Stats,
        cancellation: &Cancellation,
    ) -> crate::Result<Option<O::Output>>;

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

    fn send_residency(&self, path: &std::sync::Arc<str>, page_offset: usize, residency: &PM) {
        let mut start = 0;
        while start < residency.len() {
            let resident = residency.is_set(start);
            let mut end = start + 1;
            while end < residency.len() && residency.is_set(end) == resident {
                end += 1;
            }
            self.sink.send(Event::FileProgress {
                path: path.clone(),
                page_offset: page_offset + start,
                pages_walked: end - start,
                resident,
            });
            start = end;
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
        cancellation: &Cancellation,
    ) -> crate::Result<Option<O::Output>> {
        let path_str: std::sync::Arc<str> = path.display().to_string().into();

        let pf = match prepare_file(path, range)? {
            Some(pf) => pf,
            None => return Ok(None),
        };
        let residency: PM = crate::mincore::residency(&pf.mmap, pf.len())?;
        let pages_in_core = residency.count_filled();
        let total_pages = pf.total_pages();

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

        let reported_action = std::sync::atomic::AtomicUsize::new(0);
        let on_progress = |pages_walked: usize, action_count: usize| {
            let action = action_count;
            let delta = action.saturating_sub(reported_action.swap(action, Ordering::Relaxed));
            stats.action_pages.fetch_add(delta, Ordering::Relaxed);
            if let Some(resident) = O::EFFECT.progress_resident() {
                self.sink.send(Event::FileProgress {
                    path: path_str.clone(),
                    page_offset: 0,
                    pages_walked,
                    resident,
                });
            }
        };

        let prepared = Some((pf, residency, pages_in_core));
        let processed = full_process_file::<O, PM>(
            op,
            path,
            range,
            Some(&on_progress),
            prepared,
            cancellation,
        );
        let result = match processed {
            Ok(Some(result)) => result,
            Ok(None) => {
                self.sink.send(Event::FileDone { path: path_str });
                return Ok(None);
            }
            Err(error) => {
                self.sink.send(Event::FileDone { path: path_str });
                return Err(error);
            }
        };

        // Flush remaining action_pages not covered by the progress callback.
        let reported = reported_action.load(Ordering::Relaxed);
        let total_action = O::EFFECT.action_pages(
            result
                .pages_in_core_before()
                .unwrap_or(result.pages_in_core_after()),
            result.pages_in_core_after(),
        );
        stats
            .action_pages
            .fetch_add(total_action.saturating_sub(reported), Ordering::Relaxed);

        if O::EFFECT.has_action()
            && let Some(residency_after) = result.residency_after.as_ref()
        {
            self.send_residency(&path_str, 0, residency_after);
        }

        self.sink.send(Event::FileDone { path: path_str });

        Ok(Some(result.into_output()))
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
        cancellation: &Cancellation,
    ) -> crate::Result<Option<O::Output>> {
        let result = match counts_process_file::<O, PM>(op, path, range, cancellation)? {
            Some(result) => result,
            None => return Ok(None),
        };
        cli_record_stats::<O>(&result, stats);
        tracing::info!(
            "{}: {}/{} pages resident",
            path.display(),
            result.pages_in_core_after(),
            result.total_pages(),
        );
        Ok(Some(result.into_output()))
    }
}

pub(crate) fn full_process_file<O: Op, PM: PageMap + Sync>(
    op: &O,
    path: &Path,
    range: &FileRange,
    on_progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    prepared: Option<(PreparedFile, PM, usize)>,
    cancellation: &Cancellation,
) -> crate::Result<Option<ops::FullResult<O::Output, PM>>> {
    cancellation.check()?;
    let (pf, residency_before, pages_in_core_before) = match prepared {
        Some(tuple) => tuple,
        None => {
            let Some(pf) = prepare_file(path, range)? else {
                return Ok(None);
            };
            let residency_before: PM = crate::mincore::residency(&pf.mmap, pf.len())?;
            let pages_in_core_before = residency_before.count_filled();
            (pf, residency_before, pages_in_core_before)
        }
    };

    let ctx = FileContext::new(pf, cancellation.clone())
        .with_progress(on_progress)
        .with_residency(Some(&residency_before));

    let output = op.execute(&ctx)?;
    let total_pages = ctx.total_pages();

    let (pages_in_core_after, residency_after) = match O::EFFECT {
        ResidencyEffect::Preserve => (pages_in_core_before, None),
        ResidencyEffect::Populate => {
            let after = PM::from_bools(std::iter::repeat_n(true, total_pages));
            (total_pages, Some(after))
        }
        ResidencyEffect::EvictAdvisory => {
            let after: PM = crate::mincore::residency(ctx.mmap(), ctx.len())?;
            let count = after.count_filled();
            (count, Some(after))
        }
    };
    drop(ctx);
    let (residency_before, residency_after) = match O::EFFECT {
        ResidencyEffect::Preserve => (None, Some(residency_before)),
        ResidencyEffect::Populate | ResidencyEffect::EvictAdvisory => {
            (Some(residency_before), residency_after)
        }
    };

    Ok(Some(ops::FullResult {
        output,
        total_pages,
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
    cancellation: &Cancellation,
) -> crate::Result<Option<ops::CountsResult<O::Output>>> {
    cancellation.check()?;
    let Some(pf) = prepare_file(path, range)? else {
        return Ok(None);
    };

    let residency: Option<PM> = match O::EFFECT {
        ResidencyEffect::Populate => Some(crate::mincore::residency(&pf.mmap, pf.len())?),
        ResidencyEffect::Preserve | ResidencyEffect::EvictAdvisory => None,
    };
    let pages_in_core_before = match residency.as_ref() {
        Some(residency) => residency.count_filled(),
        None => counts_page_count::<PM>(&pf.file, &pf.mmap, pf.offset(), pf.len())?,
    };

    let ctx = FileContext::new(pf, cancellation.clone()).with_residency(residency.as_ref());

    let output = op.execute(&ctx)?;
    let total_pages = ctx.total_pages();

    let pages_in_core_after = match O::EFFECT {
        ResidencyEffect::Preserve => pages_in_core_before,
        ResidencyEffect::Populate => total_pages,
        ResidencyEffect::EvictAdvisory => {
            counts_page_count::<PM>(ctx.file(), ctx.mmap(), ctx.offset(), ctx.len())?
        }
    };

    Ok(Some(ops::CountsResult {
        output,
        total_pages,
        pages_in_core_before,
        pages_in_core_after,
    }))
}

fn cli_record_stats<O: Op>(result: &impl FileProcessed<Output = O::Output>, stats: &Stats) {
    let initial = result
        .pages_in_core_before()
        .unwrap_or(result.pages_in_core_after());
    let action = O::EFFECT.action_pages(initial, result.pages_in_core_after());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_reports_only_the_requested_range() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let page_size = *crate::pagesize::PAGE_SIZE;
        file.as_file().set_len((page_size * 3) as u64).unwrap();
        let range = FileRange::new(page_size as u64, Some(page_size as u64)).unwrap();
        let stats = Stats::new();
        let cancellation = Cancellation::new();
        let (sender, receiver) = std::sync::mpsc::channel();
        let tui = Tui::<DefaultPageMap>::new(sender);

        tui.process_one(&ops::Query, file.path(), &range, &stats, &cancellation)
            .unwrap();

        let Event::FileStart {
            total_pages,
            residency,
            ..
        } = receiver.recv().unwrap()
        else {
            panic!("expected file start event");
        };
        assert_eq!(total_pages, 1);
        assert_eq!(residency.len(), 1);
        assert_eq!(stats.total_pages.load(Ordering::Relaxed), 1);
    }
}
