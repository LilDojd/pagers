use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use ignore::WalkBuilder;

use crate::mincore::PageMap;
use crate::mode::DisplayMode;
use crate::ops::{FileRange, Op, Stats};
use crate::par::{InodeSet, SeenInodes as _};

#[cfg(feature = "rayon")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Threads {
    All,
    Exact(u16),
}

#[cfg(feature = "rayon")]
impl Default for Threads {
    fn default() -> Self {
        Self::All
    }
}

#[cfg(feature = "rayon")]
impl From<u16> for Threads {
    fn from(n: u16) -> Self {
        match n {
            0 => Self::All,
            n => Self::Exact(n),
        }
    }
}

#[cfg(feature = "rayon")]
impl std::fmt::Display for Threads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => f.write_str("0"),
            Self::Exact(n) => write!(f, "{n}"),
        }
    }
}

#[cfg(feature = "rayon")]
impl std::str::FromStr for Threads {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let n: u16 = s.parse()?;
        Ok(Self::from(n))
    }
}

#[cfg(feature = "rayon")]
impl Threads {
    pub fn num_threads(self) -> usize {
        match self {
            Self::All => 0,
            Self::Exact(n) => n as usize,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CrawlConfig {
    pub follow_symlinks: bool,
    pub single_filesystem: bool,
    pub count_hardlinks: bool,
    pub ignore_patterns: Vec<String>,
    pub filter_patterns: Vec<String>,
    pub max_file_size: Option<u64>,
    pub batch: Option<PathBuf>,
    pub nul_delim: bool,
    #[cfg(feature = "rayon")]
    pub threads: Threads,
}

pub fn crawl_and_process<O: Op, PM: PageMap + Send + Sync, D: DisplayMode<PM>>(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    op: &O,
    range: &FileRange,
    stats: &Stats,
    display: &D,
) -> crate::Result<Vec<O::Output>> {
    let seen_inodes = InodeSet::default();

    #[cfg(feature = "rayon")]
    {
        use rayon::prelude::*;

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(crawl_config.threads.num_threads())
            .build()?;

        let buf = std::thread::available_parallelism().map_or(16, |n| n.get() * 4);
        let (tx, rx) = std::sync::mpsc::sync_channel::<PathBuf>(buf);

        let outputs = pool.install(|| {
            rayon::scope(|s| {
                s.spawn(|_| {
                    collect_file_paths_streaming(paths, crawl_config, &seen_inodes, stats, tx);
                });

                rx.into_iter()
                    .par_bridge()
                    .filter_map(|path| display.process_one::<O>(op, &path, range, stats))
                    .collect::<Vec<_>>()
            })
        });

        display.finish();
        op.finish()?;
        Ok(outputs)
    }

    #[cfg(not(feature = "rayon"))]
    {
        let file_paths = collect_file_paths(paths, crawl_config, &seen_inodes, stats);
        let outputs = file_paths
            .iter()
            .filter_map(|path| display.process_one::<O>(op, path, range, stats))
            .collect();
        display.finish();
        op.finish()?;
        Ok(outputs)
    }
}

#[cfg(feature = "rayon")]
fn collect_file_paths_streaming(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    seen_inodes: &InodeSet,
    stats: &Stats,
    tx: std::sync::mpsc::SyncSender<PathBuf>,
) {
    let mut all_paths: Vec<PathBuf> = paths.to_vec();

    if let Some(batch_path) = &crawl_config.batch {
        match read_batch_paths(batch_path, crawl_config.nul_delim) {
            Ok(batch_paths) => all_paths.extend(batch_paths),
            Err(e) => tracing::warn!("batch file: {e}"),
        }
    }

    let needs_meta = crawl_config.max_file_size.is_some() || !crawl_config.count_hardlinks;

    for path in &all_paths {
        if path.is_dir() {
            stats.total_dirs.fetch_add(1, Ordering::Relaxed);
            walk_dir_entries(path, crawl_config, needs_meta, seen_inodes, |p| {
                let _ = tx.send(p);
            });
        } else if path.is_file() {
            let _ = tx.send(path.clone());
        } else {
            tracing::warn!("skipping {}: not a file or directory", path.display());
        }
    }
}

#[cfg(not(feature = "rayon"))]
fn collect_file_paths(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    seen_inodes: &InodeSet,
    stats: &Stats,
) -> Vec<PathBuf> {
    let mut all_paths: Vec<PathBuf> = paths.to_vec();

    if let Some(batch_path) = &crawl_config.batch {
        match read_batch_paths(batch_path, crawl_config.nul_delim) {
            Ok(batch_paths) => all_paths.extend(batch_paths),
            Err(e) => tracing::warn!("batch file: {e}"),
        }
    }

    let needs_meta = crawl_config.max_file_size.is_some() || !crawl_config.count_hardlinks;
    let mut file_paths = Vec::new();

    for path in &all_paths {
        if path.is_dir() {
            stats.total_dirs.fetch_add(1, Ordering::Relaxed);
            walk_dir_entries(path, crawl_config, needs_meta, seen_inodes, |p| {
                file_paths.push(p);
            });
        } else if path.is_file() {
            file_paths.push(path.clone());
        } else {
            tracing::warn!("skipping {}: not a file or directory", path.display());
        }
    }

    file_paths
}

fn walk_dir_entries(
    root: &Path,
    config: &CrawlConfig,
    needs_meta: bool,
    seen_inodes: &InodeSet,
    mut emit: impl FnMut(PathBuf),
) {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(config.follow_symlinks)
        .same_file_system(config.single_filesystem)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false);

    if !config.ignore_patterns.is_empty() || !config.filter_patterns.is_empty() {
        let mut overrides = ignore::overrides::OverrideBuilder::new(root);
        for pat in &config.ignore_patterns {
            let _ = overrides.add(&format!("!{pat}"));
        }
        for pat in &config.filter_patterns {
            let _ = overrides.add(pat);
        }
        if let Ok(ov) = overrides.build() {
            builder.overrides(ov);
        }
    }

    for entry in builder.build() {
        let Ok(entry) = entry.inspect_err(|e| tracing::warn!("{e}")) else {
            continue;
        };

        let Some(ft) = entry.file_type() else {
            continue;
        };

        if !ft.is_file() {
            continue;
        }

        let entry_path = entry.path();
        let meta = if needs_meta {
            entry_path.metadata().ok()
        } else {
            None
        };

        if let Some(max_size) = config.max_file_size
            && let Some(ref m) = meta
            && m.len() > max_size
        {
            continue;
        }

        if !config.count_hardlinks {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if let Some(ref m) = meta
                    && m.nlink() > 1
                    && seen_inodes.already_seen((m.dev(), m.ino()))
                {
                    continue;
                }
            }
        }

        emit(entry_path.to_path_buf());
    }
}

pub fn read_batch_paths(path: &Path, nul_delim: bool) -> io::Result<Vec<PathBuf>> {
    use std::os::unix::ffi::OsStrExt;

    let reader: Box<dyn BufRead> = if path == Path::new("-") {
        Box::new(io::stdin().lock())
    } else {
        Box::new(io::BufReader::new(std::fs::File::open(path)?))
    };

    let delim = if nul_delim { b'\0' } else { b'\n' };
    reader
        .split(delim)
        .filter_map(|r| match r {
            Ok(buf) if !buf.is_empty() => {
                Some(Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&buf))))
            }
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .collect()
}

#[cfg(test)]
#[cfg(feature = "rayon")]
mod tests {
    use super::*;

    #[test]
    fn threads_from_zero_is_all() {
        assert_eq!(Threads::from(0), Threads::All);
    }

    #[test]
    fn threads_from_nonzero_is_exact() {
        assert_eq!(Threads::from(4), Threads::Exact(4));
        assert_eq!(Threads::from(1), Threads::Exact(1));
    }

    #[test]
    fn threads_default_is_all() {
        assert_eq!(Threads::default(), Threads::All);
    }

    #[test]
    fn threads_num_threads_all_is_zero() {
        assert_eq!(Threads::All.num_threads(), 0);
    }

    #[test]
    fn threads_num_threads_exact() {
        assert_eq!(Threads::Exact(8).num_threads(), 8);
        assert_eq!(Threads::Exact(1).num_threads(), 1);
    }

    #[test]
    fn threads_display() {
        assert_eq!(Threads::All.to_string(), "0");
        assert_eq!(Threads::Exact(4).to_string(), "4");
    }

    #[test]
    fn threads_from_str() {
        assert_eq!("0".parse::<Threads>(), Ok(Threads::All));
        assert_eq!("4".parse::<Threads>(), Ok(Threads::Exact(4)));
        assert_eq!("1".parse::<Threads>(), Ok(Threads::Exact(1)));
        assert!("abc".parse::<Threads>().is_err());
    }

    #[test]
    fn threads_display_roundtrip() {
        for t in [Threads::All, Threads::Exact(1), Threads::Exact(8)] {
            assert_eq!(t.to_string().parse::<Threads>(), Ok(t));
        }
    }
}
