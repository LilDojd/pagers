use std::path::PathBuf;
use std::time::Instant;

use pagers_core::{crawl, mmap, ops, output};

use clap::Parser;

mod cli;
pub mod size_range;
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
) -> (ops::Stats, Vec<ops::LockedFile>) {
    if common.quiet && common.verbose > 0 {
        eprintln!("pagers: --quiet and --verbose are mutually exclusive");
        std::process::exit(1);
    }

    let (offset, max_len) = if let Some(ref range) = common.range {
        let page_size = mmap::page_size() as u64;
        let aligned = (range.start_b / page_size) * page_size;
        let max_len = range.end_b.map(|end| {
            if end <= aligned {
                eprintln!("pagers: range limits out of order after page alignment");
                std::process::exit(1);
            }
            end - aligned
        });
        (aligned, max_len)
    } else {
        (0, None)
    };

    let op_config = ops::OpConfig {
        operation,
        verbose: common.verbose,
        quiet: common.quiet,
        offset,
        max_len,
    };

    let crawl_config = crawl::CrawlConfig {
        follow_symlinks: common.follow_symlinks,
        single_filesystem: common.single_filesystem,
        count_hardlinks: common.count_hardlinks,
        ignore_patterns: common.ignore.clone(),
        filter_patterns: common.filter.clone(),
        max_file_size: common.max_file_size,
        batch: common.batch.clone(),
        nul_delim: common.nul_delim,
    };

    let stats = ops::Stats::new();
    let start = Instant::now();

    let locked = crawl::crawl_and_process(&common.paths, &crawl_config, &op_config, &stats);

    let elapsed = start.elapsed().as_secs_f64();
    let output_fmt = common.output.as_ref().map(|f| match f {
        OutputFormat::Kv => "kv",
    });

    if !common.quiet {
        output::print_summary(&stats, elapsed, mode, output_fmt);
    }

    (stats, locked)
}

impl Executable for CrawlOp<'_> {
    fn execute(&self) {
        if let Some(n) = self.threads {
            set_threads(n);
        }
        run_crawl(self.common, self.mode, self.operation);
    }
}

impl Executable for DaemonOp<'_> {
    fn execute(&self) {
        if let Some(n) = self.threads {
            set_threads(n);
        }
        let (stats, locked_files) = run_crawl(self.common, self.mode, self.operation);

        if self.lockall
            && let Err(e) = mmap::mlockall_current()
        {
            eprintln!("pagers: FATAL: {e}");
            std::process::exit(1);
        }

        if !self.common.quiet {
            let page_size = mmap::page_size() as i64;
            let total = stats.total_pages.load(std::sync::atomic::Ordering::Relaxed);
            println!(
                "LOCKED {} pages ({})",
                total,
                output::pretty_size(total * page_size)
            );
        }

        if self.daemon {
            if let Some(p) = self.pidfile
                && let Err(e) = std::fs::write(p, format!("{}\n", std::process::id()))
            {
                eprintln!("pagers: WARNING: pidfile: {e}");
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
                common: &a.common,
                operation: ops::Operation::Query,
                mode: "query",
                threads: None,
            }),
            Self::Touch(a) => Box::new(CrawlOp {
                common: &a.common,
                operation: ops::Operation::Touch(TouchParams {
                    chunk_size: a.chunk_size as usize,
                    timeout_secs: a.timeout,
                }),
                mode: "touch",
                threads: a.threads,
            }),
            Self::Evict(a) => Box::new(CrawlOp {
                common: &a.common,
                operation: ops::Operation::Evict,
                mode: "evict",
                threads: None,
            }),
            Self::Lock(a) => Box::new(DaemonOp {
                common: &a.common,
                operation: ops::Operation::Lock(TouchParams {
                    chunk_size: a.chunk_size as usize,
                    timeout_secs: a.timeout,
                }),
                mode: "lock",
                threads: a.threads,
                lockall: false,
                daemon: a.daemon,
                pidfile: &a.pidfile,
            }),
            Self::Lockall(a) => Box::new(DaemonOp {
                common: &a.common,
                operation: ops::Operation::Touch(TouchParams {
                    chunk_size: a.chunk_size as usize,
                    timeout_secs: a.timeout,
                }),
                mode: "lockall",
                threads: a.threads,
                lockall: true,
                daemon: a.daemon,
                pidfile: &a.pidfile,
            }),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    cli.command.as_operation().execute();
}
