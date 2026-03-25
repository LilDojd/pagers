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
use pagers_core::mincore::DefaultPageMap;
use pagers_core::mode;
use runop::{Cmd, Run};
use size_range::{SizeRange, parse_size};

type C<'a, O> = Cmd<'a, O, DefaultPageMap>;

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
    let cli = cli::Cli::parse();
    let output = cli.command.output();
    if output.format.is_some() && !output.is_quiet() {
        tracing::init(&output.verbosity);
    }

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

fn run(cli: cli::Cli, term: &Arc<AtomicBool>) -> Result<(), Error> {
    match cli.command {
        Command::Query(a) => {
            let quiet = a.output.is_quiet();
            let cmd = C::new(ops::Query, a.common(), term, a.output.format, quiet, None);
            match a.output.format {
                Some(_) => Run::<mode::NoDaemon, mode::CliMode>::run(cmd),
                None => Run::<mode::NoDaemon, mode::TuiMode>::run(cmd),
            }
        }
        Command::Touch(a) => {
            let quiet = a.output.is_quiet();
            let cmd = C::new(ops::Touch, a.common(), term, a.output.format, quiet, None);
            match a.output.format {
                Some(_) => Run::<mode::NoDaemon, mode::CliMode>::run(cmd),
                None => Run::<mode::NoDaemon, mode::TuiMode>::run(cmd),
            }
        }
        Command::Evict(a) => {
            let quiet = a.output.is_quiet();
            let cmd = C::new(ops::Evict, a.common(), term, a.output.format, quiet, None);
            match a.output.format {
                Some(_) => Run::<mode::NoDaemon, mode::CliMode>::run(cmd),
                None => Run::<mode::NoDaemon, mode::TuiMode>::run(cmd),
            }
        }
        Command::Lock(a) => {
            let quiet = a.output.is_quiet();
            let cmd = C::new(
                ops::Lock,
                a.common(),
                term,
                a.output.format,
                quiet,
                Some(&a.inner),
            );
            match (a.inner.daemon, a.output.format.is_some()) {
                (true, _) => Run::<mode::Daemon, mode::CliMode>::run(cmd),
                (false, true) => Run::<mode::NoDaemon, mode::CliMode>::run(cmd),
                (false, false) => Run::<mode::NoDaemon, mode::TuiMode>::run(cmd),
            }
        }
        Command::Lockall(a) => {
            let quiet = a.output.is_quiet();
            let cmd = C::new(
                ops::Lockall,
                a.common(),
                term,
                a.output.format,
                quiet,
                Some(&a.inner),
            );
            match (a.inner.daemon, a.output.format.is_some()) {
                (true, _) => Run::<mode::Daemon, mode::CliMode>::run(cmd),
                (false, true) => Run::<mode::NoDaemon, mode::CliMode>::run(cmd),
                (false, false) => Run::<mode::NoDaemon, mode::TuiMode>::run(cmd),
            }
        }
    }
}
