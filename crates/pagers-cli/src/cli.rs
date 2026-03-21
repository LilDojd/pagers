use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, ValueHint};

// Only include `size_range` for normal builds (not when compiling `build.rs` where
// the module machinery does not work)
#[cfg(pagers_normal_build)]
use crate::{SizeRange, parse_size};

/// Fast page cache control
#[derive(Parser, Debug)]
#[command(name = "pagers", version, arg_required_else_help = true)]
#[command(styles = styles())]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Args, Debug)]
pub struct WithCommon<T: clap::Args> {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(flatten)]
    pub inner: T,
}

impl<T: clap::Args> WithCommon<T> {
    pub fn common(&self) -> &CommonArgs {
        &self.common
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show page cache residency
    Query(WithCommon<()>),
    /// Touch pages into memory
    Touch(WithCommon<LoadArgs>),
    /// Evict pages from memory
    Evict(WithCommon<()>),
    /// Lock pages with mlock(2)
    Lock(WithCommon<LockInner>),
    /// Lock all pages with mlockall(2)
    Lockall(WithCommon<LockInner>),
}

impl Command {
    pub fn common(&self) -> &CommonArgs {
        match self {
            Self::Query(a) | Self::Evict(a) => a.common(),
            Self::Touch(a) => a.common(),
            Self::Lock(a) | Self::Lockall(a) => a.common(),
        }
    }

    pub fn verbosity(&self) -> &clap_verbosity_flag::Verbosity {
        &self.common().verbosity
    }
}

#[derive(Clone, clap::Args, Debug)]
pub struct FilterArgs {
    /// Ignore files matching glob pattern
    #[arg(short = 'i', long)]
    pub ignore: Vec<String>,

    /// Only process files matching glob pattern
    #[arg(short = 'I', long = "filter")]
    pub filter: Vec<String>,
}

#[derive(clap::Args, Debug)]
pub struct CommonArgs {
    /// Files or directories to process
    #[arg(required_unless_present = "batch", value_hint = ValueHint::AnyPath)]
    pub paths: Vec<PathBuf>,

    /// Follow symbolic links
    #[arg(short = 'f')]
    pub follow_symlinks: bool,

    /// Stay on same filesystem
    #[arg(short = 'F')]
    pub single_filesystem: bool,

    /// Count hardlinked copies separately
    #[arg(short = 'H')]
    pub count_hardlinks: bool,

    #[command(flatten)]
    pub verbosity: clap_verbosity_flag::Verbosity,

    /// Max file size (e.g. 4k, 100M, 1.5G)
    #[arg(short = 'm', long, value_parser = parse_size)]
    pub max_file_size: Option<u64>,

    /// Byte range (e.g. 10K-20G, 100M..500M, 0,1G)
    #[arg(short = 'p', long, value_parser = clap::value_parser!(SizeRange))]
    pub range: Option<SizeRange>,

    #[command(flatten)]
    pub filter: FilterArgs,

    /// Read paths from file (- for stdin)
    #[arg(short = 'b', long, value_hint = ValueHint::FilePath)]
    pub batch: Option<PathBuf>,

    /// NUL-delimited paths in batch mode
    #[arg(short = '0')]
    pub nul_delim: bool,

    /// Output format
    #[arg(short = 'o', long, value_enum)]
    pub output: Option<OutputFormat>,
}

#[derive(clap::Args, Debug)]
pub struct LoadArgs {
    /// Chunk size for madvise (e.g. 128M)
    #[arg(long, default_value = "128M", value_parser = parse_size)]
    pub chunk_size: u64,

    /// Max seconds to wait for madvise convergence
    #[arg(long, default_value = "30")]
    pub timeout: f64,
}

#[derive(clap::Args, Debug)]
pub struct LockInner {
    #[command(flatten)]
    pub load: LoadArgs,

    /// Run as daemon (block until signal)
    #[arg(short, long)]
    pub daemon: bool,

    /// Wait until all pages are locked (with -d)
    #[arg(short, long, requires = "daemon")]
    pub wait: bool,

    /// Write pidfile
    #[arg(short = 'P', long, value_hint = ValueHint::FilePath)]
    pub pidfile: Option<PathBuf>,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    /// Key=value pairs
    Kv,
    /// JSON output
    Json,
}

fn styles() -> clap::builder::Styles {
    use anstyle::{AnsiColor, Style};
    clap::builder::Styles::styled()
        .header(Style::new().bold().fg_color(Some(AnsiColor::Green.into())))
        .usage(Style::new().bold().fg_color(Some(AnsiColor::Green.into())))
        .literal(Style::new().fg_color(Some(AnsiColor::Cyan.into())))
        .placeholder(Style::new().fg_color(Some(AnsiColor::BrightBlack.into())))
        .error(Style::new().bold().fg_color(Some(AnsiColor::Red.into())))
        .valid(Style::new().fg_color(Some(AnsiColor::Green.into())))
        .invalid(Style::new().fg_color(Some(AnsiColor::Yellow.into())))
}
