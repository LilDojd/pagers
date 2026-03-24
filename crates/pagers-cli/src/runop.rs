use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use pagers_core::mincore::{DefaultPageMap, PageMap};
use pagers_core::mode;
use pagers_core::output::Summary;
use pagers_core::{crawl, ops};

use crate::Error;
use crate::cli::CommonArgs;

pub(crate) trait Run {
    fn run(self) -> Result<(), Error>;
}

pub(crate) struct SimpleCmd<'a, O, PM: PageMap = DefaultPageMap> {
    op: O,
    common: &'a CommonArgs,
    term: &'a Arc<AtomicBool>,
    _phantom: std::marker::PhantomData<PM>,
}

impl<'a, O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static> SimpleCmd<'a, O, PM>
where
    O::Output: 'static,
{
    pub fn new(op: O, common: &'a CommonArgs, term: &'a Arc<AtomicBool>) -> Self {
        Self {
            op,
            common,
            term,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static> Run for SimpleCmd<'_, O, PM>
where
    O::Output: 'static,
{
    fn run(self) -> Result<(), Error> {
        let use_tui = !self.common.verbosity.is_silent() && std::io::stdout().is_terminal();
        let (stats, _, elapsed) = if use_tui {
            run_tui::<O, PM>(&self.op, self.common, self.term)?
        } else {
            run_cli::<O, PM>(&self.op, self.common)?
        };
        maybe_print_summary::<O>(&stats, elapsed, self.common);
        Ok(())
    }
}

pub(crate) type RunResult<O> = Result<(Arc<ops::Stats>, Vec<O>, f64), Error>;

fn common_setup(
    common: &CommonArgs,
) -> Result<(ops::FileRange, Vec<std::path::PathBuf>, crawl::CrawlConfig), Error> {
    let (offset, max_len) = if let Some(ref range) = common.range {
        let page_size = *pagers_core::pagesize::PAGE_SIZE as u64;
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

    let stdin_is_batch = common
        .batch
        .as_deref()
        .is_some_and(|p| p == std::path::Path::new("-"));
    let mut extra_paths = common.paths.clone();
    let batch = if stdin_is_batch {
        let stdin_paths = crawl::read_batch_paths(std::path::Path::new("-"), common.nul_delim)
            .map_err(pagers_core::Error::from)?;
        extra_paths.extend(stdin_paths);
        None
    } else {
        common.batch.clone()
    };

    let crawl_config = crawl::CrawlConfig {
        follow_symlinks: common.follow_symlinks,
        single_filesystem: common.single_filesystem,
        count_hardlinks: common.count_hardlinks,
        ignore_patterns: common.filter.ignore.clone(),
        filter_patterns: common.filter.filter.clone(),
        max_file_size: common.max_file_size,
        batch,
        nul_delim: common.nul_delim,
    };

    Ok((range, extra_paths, crawl_config))
}

pub(crate) fn run_tui<O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static>(
    op: &O,
    common: &CommonArgs,
    term: &Arc<AtomicBool>,
) -> RunResult<O::Output>
where
    O::Output: 'static,
{
    let (range, extra_paths, crawl_config) = common_setup(common)?;
    let stats = Arc::new(ops::Stats::new());
    let start = Instant::now();

    let (tx, rx) = std::sync::mpsc::channel::<pagers_core::events::Event<PM>>();
    let display = mode::Tui::new(tx);

    let term_clone = Arc::clone(term);
    let stats_clone = Arc::clone(&stats);
    let tui_label = O::LABEL.to_string();
    let action_sign = O::ACTION_SIGN;
    let tui_handle = std::thread::spawn(move || {
        if let Err(e) = pagers_tui::run(rx, term_clone, stats_clone, &tui_label, action_sign, start)
        {
            ::tracing::error!("TUI error: {e}");
        }
    });

    let outputs = crawl::crawl_and_process::<O, PM, _>(
        &extra_paths,
        &crawl_config,
        op,
        &range,
        &stats,
        &display,
    );

    tui_handle.join().expect("TUI thread panicked");

    let elapsed = start.elapsed().as_secs_f64();
    Ok((stats, outputs?, elapsed))
}

pub(crate) fn run_cli<O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static>(
    op: &O,
    common: &CommonArgs,
) -> RunResult<O::Output>
where
    O::Output: 'static,
{
    let (range, extra_paths, crawl_config) = common_setup(common)?;
    let stats = Arc::new(ops::Stats::new());
    let start = Instant::now();

    let display = mode::Cli;
    let outputs = crawl::crawl_and_process::<O, PM, _>(
        &extra_paths,
        &crawl_config,
        op,
        &range,
        &stats,
        &display,
    );

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
    let summary = Summary::from_stats(stats, elapsed, O::ACTION_SIGN);
    common
        .output
        .print_summary(&summary, O::LABEL, O::ACTION_SIGN != 0);
}
