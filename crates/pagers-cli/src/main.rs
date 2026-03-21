use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pagers_core::ops;
use pagers_core::output::{OutputFormat as CoreOutputFormat, Summary};

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
    #[error("{0}")]
    Core(#[from] pagers_core::Error),
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
        Command::Query(a) => run_and_summarize(ops::Query, a.common(), term),
        Command::Touch(a) => run_and_summarize(ops::Touch, a.common(), term),
        Command::Evict(a) => run_and_summarize(ops::Evict, a.common(), term),
        Command::Lock(a) => run_lock(ops::Lock, &a, term),
        Command::Lockall(a) => run_lock(ops::Lockall, &a, term),
    }
}

fn run_and_summarize<O: RunOp>(
    op: O,
    common: &CommonArgs,
    term: &Arc<AtomicBool>,
) -> Result<(), Error>
where
    O::Output: 'static,
{
    let (stats, _, elapsed) = op.run(common, true, term)?;
    maybe_print_summary::<O>(&stats, elapsed, common);
    Ok(())
}

fn run_lock<O: Daemonize>(
    op: O,
    a: &WithCommon<LockInner>,
    term: &Arc<AtomicBool>,
) -> Result<(), Error>
where
    O::Output: 'static,
{
    if a.inner.daemon {
        op.run_daemonized(a, term)
    } else {
        op.run(a.common(), false, term)?;
        Ok(())
    }
}

fn maybe_print_summary<O: ops::Op>(stats: &ops::Stats, elapsed: f64, common: &CommonArgs) {
    use std::io::IsTerminal;

    if common.verbosity.is_silent() {
        return;
    }
    if std::io::stdout().is_terminal() {
        return;
    }
    let format = match &common.output {
        Some(OutputFormat::Kv) => CoreOutputFormat::Kv,
        Some(OutputFormat::Json) => CoreOutputFormat::Json,
        None => CoreOutputFormat::Human,
    };
    let summary = Summary::from_stats(stats, elapsed);
    format.print_summary(&summary, O::LABEL);
}
