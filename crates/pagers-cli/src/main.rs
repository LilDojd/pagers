use std::sync::Arc;
use std::time::Instant;

use pagers_core::{crawl, mmap, ops, output};

use clap::Parser;

mod cli;
pub mod size_range;
mod tracing;
use cli::*;
use size_range::{SizeRange, parse_size};

fn set_threads(n: usize) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build_global()
        .ok();
}

fn run<O: ops::Op + Send + 'static>(
    op: O,
    common: &CommonArgs,
    tui: bool,
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

    let outputs = if let Some(events_rx) = events_rx {
        let paths = common.paths.clone();
        let crawl_stats = Arc::clone(&stats);
        let crawl_handle = std::thread::spawn(move || {
            let outputs = crawl::crawl_and_process(
                &paths,
                &crawl_config,
                &op,
                &range,
                &crawl_stats,
                events_tx.as_ref(),
            );
            outputs
        });

        if let Err(e) = pagers_tui::run(events_rx) {
            ::tracing::error!("TUI error: {e}");
        }

        crawl_handle.join().expect("crawl thread panicked")
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

    let elapsed = start.elapsed().as_secs_f64();
    let output_fmt = common.output.as_ref().map(|f| match f {
        OutputFormat::Kv => "kv",
    });

    let mode = std::any::type_name::<O>()
        .rsplit("::")
        .next()
        .unwrap_or("unknown")
        .to_lowercase();

    if !common.verbosity.is_silent() {
        output::print_summary(&stats, elapsed, &mode, output_fmt);
    }

    (stats, outputs)
}

fn daemon_wait(stats: &ops::Stats, inner: &LockInner) {
    let page_size = mmap::page_size() as i64;
    let total = stats
        .total_pages
        .load(std::sync::atomic::Ordering::Relaxed);
    ::tracing::info!(
        "LOCKED {} pages ({})",
        total,
        output::pretty_size(total * page_size)
    );

    if inner.daemon {
        if let Some(p) = &inner.pidfile
            && let Err(e) = std::fs::write(p, format!("{}\n", std::process::id()))
        {
            ::tracing::warn!("pidfile: {e}");
        }

        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGINT);
            libc::sigaddset(&mut set, libc::SIGTERM);
            libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
            let mut sig: libc::c_int = 0;
            libc::sigwait(&set, &mut sig);
        }

        if let Some(p) = &inner.pidfile {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn main() {
    let cli = Cli::parse();
    tracing::init(cli.command.verbosity());

    match cli.command {
        Command::Query(a) => {
            run(ops::Query, a.common(), true);
        }
        Command::Touch(a) => {
            if let Some(n) = a.inner.threads {
                set_threads(n);
            }
            run(
                ops::Touch {
                    chunk_size: a.inner.chunk_size as usize,
                    timeout_secs: a.inner.timeout,
                },
                a.common(),
                true,
            );
        }
        Command::Evict(a) => {
            run(ops::Evict, a.common(), true);
        }
        Command::Lock(a) => {
            if let Some(n) = a.inner.load.threads {
                set_threads(n);
            }
            let (stats, _locked_files) = run(
                ops::Lock {
                    touch: ops::Touch {
                        chunk_size: a.inner.load.chunk_size as usize,
                        timeout_secs: a.inner.load.timeout,
                    },
                },
                a.common(),
                false,
            );
            daemon_wait(&stats, &a.inner);
        }
        Command::Lockall(a) => {
            if let Some(n) = a.inner.load.threads {
                set_threads(n);
            }
            let (stats, _) = run(
                ops::Touch {
                    chunk_size: a.inner.load.chunk_size as usize,
                    timeout_secs: a.inner.load.timeout,
                },
                a.common(),
                false,
            );
            if let Err(e) = mmap::mlockall_current() {
                ::tracing::error!("FATAL: {e}");
                std::process::exit(1);
            }
            daemon_wait(&stats, &a.inner);
        }
    }
}
