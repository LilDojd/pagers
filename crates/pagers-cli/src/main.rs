use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use pagers_core::{crawl, mmap, ops};

use clap::Parser;

mod cli;
mod daemon;
pub mod size_range;
mod tracing;
use cli::*;
use daemon::{daemon_hold, go_daemon};
use size_range::{SizeRange, parse_size};

fn run<O: ops::Op + Send + 'static>(
    op: O,
    common: &CommonArgs,
    tui: bool,
    term: &Arc<AtomicBool>,
) -> (Arc<ops::Stats>, Vec<O::Output>)
where
    O::Output: 'static,
{
    let (offset, max_len) = if let Some(ref range) = common.range {
        let page_size = mmap::page_size() as u64;
        let aligned = (range.start_b / page_size) * page_size;
        let max_len = range.end_b.map(|end| {
            if end <= aligned {
                ::tracing::error!("range limits out of order after page alignment");
                std::process::exit(1);
            }
            end - aligned
        });
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
    let mode = std::any::type_name::<O>()
        .rsplit("::")
        .next()
        .unwrap_or("unknown")
        .to_lowercase();

    let outputs = if let Some(events_rx) = events_rx {
        let term_clone = Arc::clone(term);
        let stats_clone = Arc::clone(&stats);
        let tui_mode = mode;
        let tui_handle = std::thread::spawn(move || {
            if let Err(e) = pagers_tui::run(events_rx, term_clone, stats_clone, tui_mode, start) {
                ::tracing::error!("TUI error: {e}");
            }
        });

        let outputs = crawl::crawl_and_process(
            &common.paths,
            &crawl_config,
            &op,
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
            &op,
            &range,
            &stats,
            events_tx.as_ref(),
        );
        drop(events_tx);
        outputs
    };

    (stats, outputs)
}

fn main() {
    let cli = Cli::parse();
    tracing::init(cli.command.verbosity());

    let term = Arc::new(AtomicBool::new(false));
    for sig in signal_hook::consts::TERM_SIGNALS {
        signal_hook::flag::register_conditional_shutdown(*sig, 1, Arc::clone(&term))
            .expect("register signal");
        signal_hook::flag::register(*sig, Arc::clone(&term)).expect("register signal");
    }

    match cli.command {
        Command::Query(a) => {
            run(ops::Query, a.common(), true, &term);
        }
        Command::Touch(a) => {
            run(
                ops::Touch {
                    chunk_size: a.inner.chunk_size as usize,
                    timeout_secs: a.inner.timeout,
                },
                a.common(),
                true,
                &term,
            );
        }
        Command::Evict(a) => {
            run(ops::Evict, a.common(), true, &term);
        }
        Command::Lock(a) => {
            let notify_fd = if a.inner.daemon {
                go_daemon(a.inner.wait)
            } else {
                None
            };
            let (stats, _locked_files) = run(
                ops::Lock {
                    touch: ops::Touch {
                        chunk_size: a.inner.load.chunk_size as usize,
                        timeout_secs: a.inner.load.timeout,
                    },
                },
                a.common(),
                false,
                &term,
            );
            if a.inner.daemon {
                daemon_hold(&stats, &a.inner, &term, notify_fd);
            }
        }
        Command::Lockall(a) => {
            let notify_fd = if a.inner.daemon {
                go_daemon(a.inner.wait)
            } else {
                None
            };
            let (stats, _) = run(
                ops::Touch {
                    chunk_size: a.inner.load.chunk_size as usize,
                    timeout_secs: a.inner.load.timeout,
                },
                a.common(),
                false,
                &term,
            );
            if let Err(e) = mmap::mlockall_current() {
                ::tracing::error!("FATAL: {e}");
                std::process::exit(1);
            }
            if a.inner.daemon {
                daemon_hold(&stats, &a.inner, &term, notify_fd);
            }
        }
    }
}
