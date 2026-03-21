use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use pagers_core::{crawl, mmap, ops, output};

use clap::Parser;

mod cli;
pub mod size_range;
mod tracing;
use cli::*;
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

fn go_daemon(wait: bool) -> Option<OwnedFd> {
    let pipe = if wait {
        Some(nix::unistd::pipe().expect("pipe"))
    } else {
        None
    };

    match unsafe { nix::unistd::fork() }.expect("fork") {
        nix::unistd::ForkResult::Parent { child: _ } => {
            if let Some((read_fd, _)) = pipe {
                wait_for_child(read_fd);
            }
            std::process::exit(0);
        }
        nix::unistd::ForkResult::Child => {
            nix::unistd::setsid().expect("setsid");
            if let Some((_, write_fd)) = pipe {
                Some(write_fd)
            } else {
                redirect_stdio();
                None
            }
        }
    }
}

fn wait_for_child(read_fd: OwnedFd) -> ! {
    use std::io::Read;
    let mut file = std::fs::File::from(read_fd);
    let mut buf = [0u8; 1];
    match file.read(&mut buf) {
        Ok(1) => std::process::exit(buf[0] as i32),
        _ => {
            ::tracing::error!("daemon shut down unexpectedly");
            std::process::exit(1);
        }
    }
}

fn redirect_stdio() {
    use std::os::fd::{FromRawFd, OwnedFd};
    if let Ok(devnull) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
    {
        for raw in [0, 1, 2] {
            let mut fd = unsafe { OwnedFd::from_raw_fd(raw) };
            let _ = nix::unistd::dup2(&devnull, &mut fd);
            std::mem::forget(fd);
        }
    }
}

fn daemon_hold(
    stats: &ops::Stats,
    inner: &LockInner,
    term: &AtomicBool,
    notify_fd: Option<OwnedFd>,
) {
    if let Some(p) = &inner.pidfile
        && let Err(e) = std::fs::write(p, format!("{}\n", std::process::id()))
    {
        ::tracing::warn!("pidfile: {e}");
    }

    let page_size = mmap::page_size() as i64;
    let total = stats.total_pages.load(std::sync::atomic::Ordering::Relaxed);
    ::tracing::info!(
        "LOCKED {} pages ({})",
        total,
        output::pretty_size(total * page_size)
    );

    if let Some(fd) = notify_fd {
        use std::io::Write;
        let mut file = std::fs::File::from(fd);
        let _ = file.write_all(&[0u8]);
    }

    while !term.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    if let Some(p) = &inner.pidfile {
        let _ = std::fs::remove_file(p);
    }
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
