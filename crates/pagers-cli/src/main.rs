use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pagers_core::ops;

use clap::Parser;

mod cli;
mod daemon;
mod output;
mod runop;
pub mod size_range;
mod tracing;
use cli::*;
use daemon::DaemonCmd;
use pagers_core::mincore::DefaultPageMap;
use runop::{Run, SimpleCmd};
use size_range::{SizeRange, parse_size};

type Cmd<'a, O> = SimpleCmd<'a, O, DefaultPageMap>;
type Daemon<'a, O> = DaemonCmd<'a, O, DefaultPageMap>;

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
        Command::Query(a) => Cmd::new(ops::Query, a.common(), term).run(),
        Command::Touch(a) => Cmd::new(ops::Touch, a.common(), term).run(),
        Command::Evict(a) => Cmd::new(ops::Evict, a.common(), term).run(),
        Command::Lock(a) => Daemon::new(ops::Lock, &a, term).run(),
        Command::Lockall(a) => Daemon::new(ops::Lockall, &a, term).run(),
    }
}
