use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, ValueHint};

/// Fast page cache control
#[derive(Parser, Debug)]
#[command(name = "pagers", version, arg_required_else_help = true)]
#[command(styles = styles())]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show page cache residency
    Query(QueryArgs),
    /// Touch pages into memory
    Touch(TouchArgs),
    /// Evict pages from memory
    Evict(QueryArgs),
    /// Lock pages with mlock(2)
    Lock(LockArgs),
    /// Lock all pages with mlockall(2)
    Lockall(LockArgs),
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

    /// Quiet mode
    #[arg(short, long)]
    pub quiet: bool,

    /// Verbose (repeatable)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Max file size (e.g. 4k, 100M, 1.5G)
    #[arg(short = 'm', long, value_parser = parse_size)]
    pub max_file_size: Option<u64>,

    /// Byte range (e.g. 0-1G, 100M-500M)
    #[arg(short = 'p', long)]
    pub range: Option<String>,

    /// Ignore files matching glob pattern
    #[arg(short = 'i', long)]
    pub ignore: Vec<String>,

    /// Only process files matching glob pattern
    #[arg(short = 'I', long = "filter")]
    pub filter: Vec<String>,

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
pub struct QueryArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(clap::Args, Debug)]
pub struct TouchArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Chunk size for parallel madvise (e.g. 128M)
    #[arg(long, default_value = "128M", value_parser = parse_size)]
    pub chunk_size: u64,

    /// Max seconds to wait for madvise convergence
    #[arg(long, default_value = "30")]
    pub timeout: u64,

    /// Thread pool size (default: num CPUs)
    #[arg(long)]
    pub threads: Option<usize>,
}

#[derive(clap::Args, Debug)]
pub struct LockArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Chunk size for parallel madvise (e.g. 128M)
    #[arg(long, default_value = "128M", value_parser = parse_size)]
    pub chunk_size: u64,

    /// Max seconds to wait for madvise convergence
    #[arg(long, default_value = "30")]
    pub timeout: u64,

    /// Thread pool size (default: num CPUs)
    #[arg(long)]
    pub threads: Option<usize>,

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
}

pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size".to_string());
    }

    let (num_str, multiplier) = match s.as_bytes().last() {
        Some(b'k' | b'K') => (&s[..s.len() - 1], 1024u64),
        Some(b'm' | b'M') => (&s[..s.len() - 1], 1024 * 1024),
        Some(b'g' | b'G') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1u64),
    };

    let val: f64 = num_str
        .parse()
        .map_err(|e| format!("bad size '{s}': {e}"))?;
    if val < 0.0 {
        return Err("size must be positive".to_string());
    }

    Ok((val * multiplier as f64) as u64)
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
