use std::sync::Arc;
use std::time::Instant;

use pagers_core::Cancellation;
use pagers_core::mincore::PageMap;
use pagers_core::mode;
use pagers_core::output::Summary;
use pagers_core::{crawl, ops};

use crate::Error;
use crate::cli::{CommonArgs, LockInner, OutputFormatArg};
use crate::daemon;

pub(crate) fn run_tui_command<
    O: ops::Op + Send + 'static,
    PM: PageMap + Clone + Send + Sync + 'static,
>(
    op: &O,
    common: &CommonArgs,
    cancellation: &Cancellation,
    lock: Option<&LockInner>,
) -> Result<(), Error>
where
    O::Output: 'static,
{
    install_signal_handler(cancellation)?;
    let (stats, _outputs, _) = run_tui::<O, PM>(op, common, cancellation)?;
    if let Some(lock) = lock {
        daemon::hold(&stats, lock, cancellation, None);
    }
    Ok(())
}

pub(crate) fn run_cli_command<O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static>(
    op: &O,
    common: &CommonArgs,
    cancellation: &Cancellation,
    format: Option<OutputFormatArg>,
    quiet: bool,
    lock: Option<&LockInner>,
) -> Result<(), Error>
where
    O::Output: 'static,
{
    install_signal_handler(cancellation)?;
    let (stats, _outputs, elapsed) = run_cli::<O, PM>(op, common, cancellation)?;
    if !quiet {
        print_summary::<O>(&stats, elapsed, format.unwrap_or_default());
    }
    if let Some(lock) = lock {
        daemon::hold(&stats, lock, cancellation, None);
    }
    Ok(())
}

pub(crate) fn run_daemon_command<O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static>(
    op: &O,
    common: &CommonArgs,
    cancellation: &Cancellation,
    lock: &LockInner,
) -> Result<(), Error>
where
    O::Output: 'static,
{
    let setup = common_setup(common)?;
    match daemon::go_daemon(lock.wait)? {
        daemon::ForkOutcome::Parent => Ok(()),
        daemon::ForkOutcome::Child(notify_fd) => {
            install_signal_handler(cancellation)?;
            let (stats, _locks, _) = match run_cli_with_setup::<O, PM>(op, setup, cancellation) {
                Ok(result) => result,
                Err(error) => {
                    tracing::error!("{error}");
                    daemon::notify_and_redirect(notify_fd, 1);
                    return Err(error);
                }
            };
            daemon::hold(&stats, lock, cancellation, notify_fd);
            Ok(())
        }
    }
}

fn install_signal_handler(cancellation: &Cancellation) -> Result<(), Error> {
    let mut signals = signal_hook::iterator::Signals::new(signal_hook::consts::TERM_SIGNALS)
        .map_err(pagers_core::Error::from)?;
    let cancellation = cancellation.clone();
    std::thread::spawn(move || {
        for (index, _) in signals.forever().enumerate() {
            if index == 0 {
                cancellation.cancel();
            } else {
                std::process::exit(1);
            }
        }
    });
    Ok(())
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

    let range = ops::FileRange::new(offset, max_len)?;

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
        threads: common.threads,
    };
    crawl::validate_patterns(&crawl_config)?;

    Ok((range, extra_paths, crawl_config))
}

pub(crate) fn run_tui<O: ops::Op + Send + 'static, PM: PageMap + Clone + Send + Sync + 'static>(
    op: &O,
    common: &CommonArgs,
    cancellation: &Cancellation,
) -> RunResult<O::Output>
where
    O::Output: 'static,
{
    let (range, extra_paths, crawl_config) = common_setup(common)?;
    let stats = Arc::new(ops::Stats::new());
    let start = Instant::now();

    let (tx, rx) = std::sync::mpsc::channel::<pagers_core::events::Event<PM>>();
    let display = mode::Tui::new(tx);

    let tui_cancellation = cancellation.clone();
    let stats_clone = Arc::clone(&stats);
    let tui_label = O::LABEL.to_string();
    let action_sign = O::EFFECT.action_sign();
    let tui_handle = std::thread::spawn(move || {
        let cancel_on_error = tui_cancellation.clone();
        let result = pagers_tui::run(
            rx,
            tui_cancellation,
            stats_clone,
            &tui_label,
            action_sign,
            start,
        );
        if result.is_err() {
            cancel_on_error.cancel();
        }
        result
    });

    let outputs = crawl::crawl_and_process::<O, PM, _>(
        &extra_paths,
        &crawl_config,
        op,
        &range,
        &stats,
        &display,
        cancellation,
    );

    tui_handle
        .join()
        .map_err(|_| Error::TuiPanic)?
        .map_err(Error::Tui)?;

    let elapsed = start.elapsed().as_secs_f64();
    Ok((stats, outputs?, elapsed))
}

pub(crate) fn run_cli<O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static>(
    op: &O,
    common: &CommonArgs,
    cancellation: &Cancellation,
) -> RunResult<O::Output>
where
    O::Output: 'static,
{
    run_cli_with_setup::<O, PM>(op, common_setup(common)?, cancellation)
}

fn run_cli_with_setup<O: ops::Op + Send + 'static, PM: PageMap + Send + Sync + 'static>(
    op: &O,
    (range, extra_paths, crawl_config): (
        ops::FileRange,
        Vec<std::path::PathBuf>,
        crawl::CrawlConfig,
    ),
    cancellation: &Cancellation,
) -> RunResult<O::Output>
where
    O::Output: 'static,
{
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
        cancellation,
    );

    let elapsed = start.elapsed().as_secs_f64();
    Ok((stats, outputs?, elapsed))
}

fn print_summary<O: ops::Op>(stats: &ops::Stats, elapsed: f64, fmt: OutputFormatArg) {
    let summary = Summary::from_stats(stats, elapsed, O::EFFECT.action_sign());
    fmt.print_summary(&summary, O::LABEL, O::EFFECT.has_action());
}
