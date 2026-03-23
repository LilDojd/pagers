use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use pagers_core::output::Summary;
use pagers_core::{crawl, mmap, ops};

use crate::Error;
use crate::cli::CommonArgs;

pub(crate) trait Run {
    fn run(self) -> Result<(), Error>;
}

pub(crate) struct SimpleCmd<'a, O> {
    op: O,
    common: &'a CommonArgs,
    term: &'a Arc<AtomicBool>,
}

impl<'a, O: ops::Op + Send + 'static> SimpleCmd<'a, O>
where
    O::Output: 'static,
{
    pub fn new(op: O, common: &'a CommonArgs, term: &'a Arc<AtomicBool>) -> Self {
        Self { op, common, term }
    }
}

impl<O: ops::Op + Send + 'static> Run for SimpleCmd<'_, O>
where
    O::Output: 'static,
{
    fn run(self) -> Result<(), Error> {
        let (stats, _, elapsed) = run_op(&self.op, self.common, true, self.term)?;
        maybe_print_summary::<O>(&stats, elapsed, self.common);
        Ok(())
    }
}

pub(crate) type RunResult<O> = Result<(Arc<ops::Stats>, Vec<O>, f64), Error>;

pub(crate) fn run_op<O: ops::Op + Send + 'static>(
    op: &O,
    common: &CommonArgs,
    tui: bool,
    term: &Arc<AtomicBool>,
) -> RunResult<O::Output>
where
    O::Output: 'static,
{
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

    let use_tui = tui && !common.verbosity.is_silent() && std::io::stdout().is_terminal();
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
        let tui_label = O::LABEL.to_string();
        std::thread::spawn(move || {
            if let Err(e) = pagers_tui::run(rx, term_clone, stats_clone, &tui_label, start) {
                ::tracing::error!("TUI error: {e}");
            }
        })
    });

    let outputs = crawl::crawl_and_process(
        &common.paths,
        &crawl_config,
        op,
        &range,
        &stats,
        events_tx,
    );

    if let Some(handle) = tui_handle {
        handle.join().expect("TUI thread panicked");
    }

    let elapsed = start.elapsed().as_secs_f64();
    Ok((stats, outputs?, elapsed))
}

fn maybe_print_summary<O: ops::Op>(stats: &ops::Stats, elapsed: f64, common: &CommonArgs) {
    if common.verbosity.is_silent() {
        return;
    }
    if std::io::stdout().is_terminal() {
        return;
    }
    let summary = Summary::from_stats(stats, elapsed);
    common.output.print_summary(&summary, O::LABEL);
}
