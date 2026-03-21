use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pagers_core::ops;

use clap::Parser;

mod cli;
mod daemon;
mod runop;
pub mod size_range;
mod tracing;
use cli::*;
use daemon::Daemonize;
use runop::RunOp;
use size_range::{SizeRange, parse_size};

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("range limits out of order after page alignment")]
    RangeOrder,
    #[error("daemon shut down unexpectedly")]
    DaemonShutdown,
    #[error("daemon child exited with status {0}")]
    DaemonExit(u8),
    #[error("{0}")]
    Nix(#[from] nix::errno::Errno),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing::init(cli.command.verbosity());

    let term = Arc::new(AtomicBool::new(false));
    for sig in signal_hook::consts::TERM_SIGNALS {
        signal_hook::flag::register_conditional_shutdown(*sig, 1, Arc::clone(&term))
            .expect("register signal");
        signal_hook::flag::register(*sig, Arc::clone(&term)).expect("register signal");
    }

    match run(cli, &term) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::DaemonExit(code)) => ExitCode::from(code),
        Err(e) => {
            ::tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli, term: &Arc<AtomicBool>) -> Result<(), Error> {
    match cli.command {
        Command::Query(a) => {
            let common = a.common();
            let (stats, _, elapsed) = ops::Query.run(common, true, term)?;
            maybe_print_summary(&stats, elapsed, "query", common);
        }
        Command::Touch(a) => {
            let common = a.common();
            let (stats, _, elapsed) = ops::Touch {
                chunk_size: a.inner.chunk_size as usize,
                timeout_secs: a.inner.timeout,
            }
            .run(common, true, term)?;
            maybe_print_summary(&stats, elapsed, "touch", common);
        }
        Command::Evict(a) => {
            let common = a.common();
            let (stats, _, elapsed) = ops::Evict.run(common, true, term)?;
            maybe_print_summary(&stats, elapsed, "evict", common);
        }
        Command::Lock(a) => {
            let op = ops::Lock::from_args(&a.inner);
            if a.inner.daemon {
                op.run_daemonized(&a, term)?;
            } else {
                op.run(a.common(), false, term)?;
            }
        }
        Command::Lockall(a) => {
            let op = ops::Lockall::from_args(&a.inner);
            if a.inner.daemon {
                op.run_daemonized(&a, term)?;
            } else {
                op.run(a.common(), false, term)?;
            }
        }
    }
    Ok(())
}

fn maybe_print_summary(stats: &ops::Stats, elapsed: f64, mode: &str, common: &CommonArgs) {
    use std::io::IsTerminal;

    if common.verbosity.is_silent() {
        return;
    }
    if std::io::stdout().is_terminal() {
        return;
    }
    let format_str = common.output.as_ref().map(|f| match f {
        OutputFormat::Kv => "kv",
    });
    pagers_core::output::print_summary(stats, elapsed, mode, format_str);
}
