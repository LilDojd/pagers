use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pagers_core::ops;
use pagers_core::output::{self, Mode, OutputFormat as CoreOutputFormat};

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
        Command::Query(a) => {
            let common = a.common();
            let (stats, _, elapsed) = ops::Query.run(common, Mode::Query, true, term)?;
            maybe_print_summary(&stats, elapsed, Mode::Query, common);
        }
        Command::Touch(a) => {
            let common = a.common();
            let (stats, _, elapsed) =
                ops::Touch.run(common, Mode::Touch, true, term)?;
            maybe_print_summary(&stats, elapsed, Mode::Touch, common);
        }
        Command::Evict(a) => {
            let common = a.common();
            let (stats, _, elapsed) = ops::Evict.run(common, Mode::Evict, true, term)?;
            maybe_print_summary(&stats, elapsed, Mode::Evict, common);
        }
        Command::Lock(a) => {
            if a.inner.daemon {
                ops::Lock.run_daemonized(&a, term)?;
            } else {
                ops::Lock.run(a.common(), Mode::Lock, false, term)?;
            }
        }
        Command::Lockall(a) => {
            if a.inner.daemon {
                ops::Lockall.run_daemonized(&a, term)?;
            } else {
                ops::Lockall.run(a.common(), Mode::Lockall, false, term)?;
            }
        }
    }
    Ok(())
}

fn maybe_print_summary(stats: &ops::Stats, elapsed: f64, mode: Mode, common: &CommonArgs) {
    use std::io::IsTerminal;

    if common.verbosity.is_silent() {
        return;
    }
    if std::io::stdout().is_terminal() {
        return;
    }
    let format = resolve_output_format(&common.output);
    output::print_summary(stats, elapsed, mode, format);
}

fn resolve_output_format(cli_format: &Option<OutputFormat>) -> CoreOutputFormat {
    match cli_format {
        Some(OutputFormat::Kv) => CoreOutputFormat::Kv,
        Some(OutputFormat::Json) => CoreOutputFormat::Json,
        None => CoreOutputFormat::Pretty,
    }
}
