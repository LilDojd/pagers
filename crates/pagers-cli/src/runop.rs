use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use pagers_core::{crawl, mmap, ops};

use crate::Error;
use crate::cli::CommonArgs;

pub(crate) type RunResult<O> = Result<(Arc<ops::Stats>, Vec<O>, f64), Error>;

pub(crate) trait RunOp: ops::Op + Send + Sized + 'static
where
    Self::Output: 'static,
{
    /// Run the operation. Returns (stats, outputs, elapsed_secs).
    /// When TUI is active, summary is printed by TUI; caller should only print for non-TUI.
    fn run(
        &self,
        common: &CommonArgs,
        tui: bool,
        term: &Arc<AtomicBool>,
    ) -> RunResult<Self::Output> {
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

        let use_tui = tui
            && !common.verbosity.is_silent()
            && std::io::stdout().is_terminal();
        let (events_tx, events_rx) = if use_tui {
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

        let tui_handle = events_rx.map(|rx| {
            let term_clone = Arc::clone(term);
            let stats_clone = Arc::clone(&stats);
            let tui_label = Self::LABEL.to_string();
            std::thread::spawn(move || {
                if let Err(e) =
                    pagers_tui::run(rx, term_clone, stats_clone, &tui_label, start)
                {
                    ::tracing::error!("TUI error: {e}");
                }
            })
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

        if let Some(handle) = tui_handle {
            handle.join().expect("TUI thread panicked");
        }

        let elapsed = start.elapsed().as_secs_f64();
        Ok((stats, outputs?, elapsed))
    }
}

impl<O> RunOp for O
where
    O: ops::Op + Send + 'static,
    O::Output: 'static,
{
}
