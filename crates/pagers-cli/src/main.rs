use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use pagers_core::{crawl, mmap, ops, output};

use clap::Parser;

mod cli;
pub mod size_range;
mod tracing;
use cli::*;
use size_range::{SizeRange, parse_size};

trait Executable {
    fn execute(&self);
}

struct CrawlOp<'a> {
    common: &'a CommonArgs,
    operation: ops::Operation,
    mode: &'static str,
    threads: Option<usize>,
    tui: bool,
}

struct DaemonOp<'a> {
    common: &'a CommonArgs,
    operation: ops::Operation,
    mode: &'static str,
    threads: Option<usize>,
    lockall: bool,
    daemon: bool,
    pidfile: &'a Option<PathBuf>,
}

fn set_threads(n: usize) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build_global()
        .ok();
}

fn run_crawl(
    common: &CommonArgs,
    mode: &str,
    operation: ops::Operation,
    tui: bool,
) -> (Arc<ops::Stats>, Vec<ops::LockedFile>) {
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

    let (events_tx, events_rx) = if tui && !common.verbosity.is_silent() {
        let (tx, rx) = std::sync::mpsc::channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let op_config = ops::OpConfig {
        operation,
        offset,
        max_len,
        events: events_tx,
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

    let locked = if let Some(events_rx) = events_rx {
        // Spawn crawl on background thread, run TUI on main thread
        let paths = common.paths.clone();
        let crawl_stats = Arc::clone(&stats);
        let crawl_handle = std::thread::spawn(move || {
            let locked = crawl::crawl_and_process(&paths, &crawl_config, &op_config, &crawl_stats);
            // op_config (holding events_tx) drops here, signalling TUI to exit
            locked
        });

        if let Err(e) = pagers_tui::run(events_rx) {
            ::tracing::error!("TUI error: {e}");
        }

        crawl_handle.join().expect("crawl thread panicked")
    } else {
        let locked = crawl::crawl_and_process(&common.paths, &crawl_config, &op_config, &stats);
        drop(op_config);
        locked
    };

    let elapsed = start.elapsed().as_secs_f64();
    let output_fmt = common.output.as_ref().map(|f| match f {
        OutputFormat::Kv => "kv",
    });

    if !common.verbosity.is_silent() {
        output::print_summary(&stats, elapsed, mode, output_fmt);
    }

    (stats, locked)
}

impl Executable for CrawlOp<'_> {
    fn execute(&self) {
        if let Some(n) = self.threads {
            set_threads(n);
        }
        run_crawl(self.common, self.mode, self.operation, self.tui);
    }
}

impl Executable for DaemonOp<'_> {
    fn execute(&self) {
        if let Some(n) = self.threads {
            set_threads(n);
        }
        let (stats, locked_files) = run_crawl(self.common, self.mode, self.operation, false);

        if self.lockall
            && let Err(e) = mmap::mlockall_current()
        {
            ::tracing::error!("FATAL: {e}");
            std::process::exit(1);
        }

        {
            let page_size = mmap::page_size() as i64;
            let total = stats.total_pages.load(std::sync::atomic::Ordering::Relaxed);
            ::tracing::info!(
                "LOCKED {} pages ({})",
                total,
                output::pretty_size(total * page_size)
            );
        }

        if self.daemon {
            if let Some(p) = self.pidfile
                && let Err(e) = std::fs::write(p, format!("{}\n", std::process::id()))
            {
                ::tracing::warn!("pidfile: {e}");
            }

            let _keep = locked_files;
            let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
            unsafe {
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGINT);
                libc::sigaddset(&mut set, libc::SIGTERM);
                libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
                let mut sig: libc::c_int = 0;
                libc::sigwait(&set, &mut sig);
            }

            if let Some(p) = self.pidfile {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

impl Command {
    fn as_operation(&self) -> Box<dyn Executable + '_> {
        use ops::TouchParams;
        match self {
            Self::Query(a) => Box::new(CrawlOp {
                common: a.common(),
                operation: ops::Operation::Query,
                mode: "query",
                threads: None,
                tui: true,
            }),
            Self::Touch(a) => Box::new(CrawlOp {
                common: a.common(),
                operation: ops::Operation::Touch(TouchParams {
                    chunk_size: a.inner.chunk_size as usize,
                    timeout_secs: a.inner.timeout,
                }),
                mode: "touch",
                threads: a.inner.threads,
                tui: true,
            }),
            Self::Evict(a) => Box::new(CrawlOp {
                common: a.common(),
                operation: ops::Operation::Evict,
                mode: "evict",
                threads: None,
                tui: true,
            }),
            Self::Lock(a) => Box::new(DaemonOp {
                common: a.common(),
                operation: ops::Operation::Lock(TouchParams {
                    chunk_size: a.inner.load.chunk_size as usize,
                    timeout_secs: a.inner.load.timeout,
                }),
                mode: "lock",
                threads: a.inner.load.threads,
                lockall: false,
                daemon: a.inner.daemon,
                pidfile: &a.inner.pidfile,
            }),
            Self::Lockall(a) => Box::new(DaemonOp {
                common: a.common(),
                operation: ops::Operation::Touch(TouchParams {
                    chunk_size: a.inner.load.chunk_size as usize,
                    timeout_secs: a.inner.load.timeout,
                }),
                mode: "lockall",
                threads: a.inner.load.threads,
                lockall: true,
                daemon: a.inner.daemon,
                pidfile: &a.inner.pidfile,
            }),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    tracing::init(&cli.command.verbosity());
    cli.command.as_operation().execute();
}
