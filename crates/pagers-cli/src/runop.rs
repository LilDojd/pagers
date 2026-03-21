use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use pagers_core::{crawl, mmap, ops};

use crate::Error;
use crate::cli::CommonArgs;

pub(crate) trait RunOp: ops::Op + Send + Sized + 'static
where
    Self::Output: 'static,
{
    fn run(
        &self,
        common: &CommonArgs,
        tui: bool,
        term: &Arc<AtomicBool>,
    ) -> Result<(Arc<ops::Stats>, Vec<Self::Output>), Error> {
        let (offset, max_len) = if let Some(ref range) = common.range {
            let page_size = mmap::page_size() as u64;
            let aligned = (range.start_b / page_size) * page_size;
            let max_len = match range.end_b {
                Some(end) if end <= aligned => return Err(Error::RangeOrder),
                Some(end) => Some(end - aligned),
                None => None,
            };
            (aligned, max_len)
        } else {
            (0, None)
        };

        let range = ops::FileRange { offset, max_len };

        let (events_tx, events_rx) = if tui && !common.verbosity.is_silent() {
            let (tx, rx) = std::sync::mpsc::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        let crawl_config = crawl::CrawlConfig {
            follow_symlinks: common.follow_symlinks,
            single_filesystem: common.single_filesystem,
            count_hardlinks: common.count_hardlinks,
            ignore_patterns: common.filter.ignore.clone(),
            filter_patterns: common.filter.filter.clone(),
            max_file_size: common.max_file_size,
            batch: common.batch.clone(),
            nul_delim: common.nul_delim,
        };

        let stats = Arc::new(ops::Stats::new());
        let start = Instant::now();
        let mode = std::any::type_name::<Self>()
            .rsplit("::")
            .next()
            .unwrap_or("unknown")
            .to_lowercase();

        let outputs = if let Some(events_rx) = events_rx {
            let term_clone = Arc::clone(term);
            let stats_clone = Arc::clone(&stats);
            let tui_mode = mode;
            let tui_handle = std::thread::spawn(move || {
                if let Err(e) =
                    pagers_tui::run(events_rx, term_clone, stats_clone, tui_mode, start)
                {
                    ::tracing::error!("TUI error: {e}");
                }
            });

            let outputs = crawl::crawl_and_process(
                &common.paths,
                &crawl_config,
                self,
                &range,
                &stats,
                events_tx.as_ref(),
            );
            drop(events_tx);

            tui_handle.join().expect("TUI thread panicked");
            outputs
        } else {
            let outputs = crawl::crawl_and_process(
                &common.paths,
                &crawl_config,
                self,
                &range,
                &stats,
                events_tx.as_ref(),
            );
            drop(events_tx);
            outputs
        };

        Ok((stats, outputs))
    }
}

impl<O> RunOp for O
where
    O: ops::Op + Send + 'static,
    O::Output: 'static,
{
}
