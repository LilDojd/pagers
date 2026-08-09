use std::io::IsTerminal as _;
use std::process::ExitCode;

use pagers_core::Cancellation;
use pagers_core::ops;

use clap::Parser;

mod cli;
mod daemon;
mod output;
mod runop;
pub mod size_range;
mod tracing;
use cli::*;
use pagers_core::mincore::DefaultPageMap;
use runop::{run_cli_command, run_daemon_command, run_tui_command};
use size_range::{SizeRange, parse_size};

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("range limits out of order after page alignment")]
    RangeOrder,
    #[error("daemon shut down unexpectedly")]
    DaemonShutdown,
    #[error("daemon child exited with status {0}")]
    DaemonExit(u8),
    #[error("TUI: {0}")]
    Tui(#[source] std::io::Error),
    #[error("TUI worker thread panicked")]
    TuiPanic,
    #[error("{0}")]
    Nix(#[from] nix::errno::Errno),
    #[error("{0}")]
    Core(#[from] pagers_core::Error),
}

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    let output = cli.command.output();
    if output.format.is_some() || output.is_quiet() {
        tracing::init(&output.verbosity);
    }

    let cancellation = Cancellation::new();

    match run(cli, &cancellation) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::DaemonExit(code)) => ExitCode::from(code),
        Err(e) => {
            ::tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: cli::Cli, cancellation: &Cancellation) -> Result<(), Error> {
    match cli.command {
        Command::Query(ref a) => run_simple(ops::Query, a, cancellation),
        Command::Touch(ref a) => run_simple(ops::Touch, a, cancellation),
        Command::Evict(ref a) => run_simple(ops::Evict, a, cancellation),
        Command::Lock(ref a) => run_lockable(ops::Lock, a, cancellation),
        Command::Lockall(ref a) => run_lockable(ops::Lockall, a, cancellation),
    }
}

fn run_simple<O: ops::Op + Send + 'static>(
    op: O,
    a: &WithCommon<()>,
    cancellation: &Cancellation,
) -> Result<(), Error>
where
    O::Output: 'static,
{
    let quiet = a.output.is_quiet();
    if a.output.format.is_some() || quiet || !std::io::stdout().is_terminal() {
        run_cli_command::<_, DefaultPageMap>(
            &op,
            &a.common,
            cancellation,
            a.output.format,
            quiet,
            None,
        )
    } else {
        run_tui_command::<_, DefaultPageMap>(&op, &a.common, cancellation, None)
    }
}

fn run_lockable<O: ops::Op + Send + 'static>(
    op: O,
    a: &WithCommon<LockInner>,
    cancellation: &Cancellation,
) -> Result<(), Error>
where
    O::Output: 'static,
{
    let quiet = a.output.is_quiet();
    let use_cli = a.output.format.is_some() || quiet || !std::io::stdout().is_terminal();
    match (a.inner.daemon, use_cli) {
        (true, _) => {
            run_daemon_command::<_, DefaultPageMap>(&op, &a.common, cancellation, &a.inner)
        }
        (_, true) => run_cli_command::<_, DefaultPageMap>(
            &op,
            &a.common,
            cancellation,
            a.output.format,
            quiet,
            Some(&a.inner),
        ),
        (false, false) => {
            run_tui_command::<_, DefaultPageMap>(&op, &a.common, cancellation, Some(&a.inner))
        }
    }
}
