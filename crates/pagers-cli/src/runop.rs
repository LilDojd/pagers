use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use pagers_core::mincore::{DefaultPageMap, PageMap};
use pagers_core::mode;
use pagers_core::output::Summary;
use pagers_core::{crawl, ops};

use crate::Error;
use crate::cli::{CommonArgs, LockInner, OutputFormatArg};
use crate::daemon;

pub(crate) trait Run<D, M> {
    fn run(self) -> Result<(), Error>;
}

pub(crate) struct Cmd<'a, O, PM: PageMap = DefaultPageMap> {
    pub op: O,
    pub common: &'a CommonArgs,
    pub term: &'a Arc<AtomicBool>,
    pub format: Option<OutputFormatArg>,
    pub quiet: bool,
    pub lock: Option<&'a LockInner>,
    _phantom: std::marker::PhantomData<PM>,
}

impl<'a, O, PM: PageMap> Cmd<'a, O, PM> {
    pub fn new(
        op: O,
        common: &'a CommonArgs,
        term: &'a Arc<AtomicBool>,
        format: Option<OutputFormatArg>,
        quiet: bool,
        lock: Option<&'a LockInner>,
    ) -> Self {
        Self {
            op,
            common,
            term,
            format,
            quiet,
            lock,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<O: ops::Op + Send + 'static, PM: PageMap + Clone + Send + Sync + 'static>
    Run<mode::NoDaemon, mode::TuiMode> for Cmd<'_, O, PM>
where
    O::Output: 'static,
{
    fn run(self) -> Result<(), Error> {
        let (stats, _outputs, _) = run_tui::<O, PM>(&self.op, self.common, self.term)?;
        if let Some(lock) = self.lock {
            daemon::hold(&stats, lock, self.term, None);
        }
        Ok(())
    }
}

impl<O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static>
    Run<mode::NoDaemon, mode::CliMode> for Cmd<'_, O, PM>
where
    O::Output: 'static,
{
    fn run(self) -> Result<(), Error> {
        let (stats, _, elapsed) = run_cli::<O, PM>(&self.op, self.common)?;
        if !self.quiet {
            print_summary::<O>(&stats, elapsed, self.format.unwrap_or_default());
        }
        if let Some(lock) = self.lock {
            daemon::hold(&stats, lock, self.term, None);
        }
        Ok(())
    }
}

impl<O: ops::Op + Send + 'static, PM: PageMap + Clone + Send + Sync + 'static>
    Run<mode::Daemon, mode::CliMode> for Cmd<'_, O, PM>
where
    O::Output: 'static,
{
    fn run(self) -> Result<(), Error> {
        let lock = self.lock.expect("daemon requires LockInner");
        match daemon::go_daemon(lock.wait)? {
            daemon::ForkOutcome::Parent => Ok(()),
            daemon::ForkOutcome::Child(notify_fd) => {
                let (stats, _locks, _) = run_cli::<O, PM>(&self.op, self.common)?;
                daemon::hold(&stats, lock, self.term, notify_fd);
                Ok(())
            }
        }
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
        #[cfg(feature = "rayon")]
        threads: common.threads,
    };

    Ok((range, extra_paths, crawl_config))
}

pub(crate) fn run_tui<O: ops::Op + Send + 'static, PM: PageMap + Clone + Send + Sync + 'static>(
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

fn print_summary<O: ops::Op>(stats: &ops::Stats, elapsed: f64, fmt: OutputFormatArg) {
    let summary = Summary::from_stats(stats, elapsed, O::ACTION_SIGN);
    fmt.print_summary(&summary, O::LABEL, O::ACTION_SIGN != 0);
}
