use std::collections::{HashSet, VecDeque};
use std::io::{self, BufRead};
use std::num::{NonZeroU16, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use dua_core::Order;

use crate::Cancellation;
use crate::mincore::PageMap;
use crate::mode::DisplayMode;
use crate::ops::{FileRange, Op, Stats};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Threads {
    #[default]
    All,
    Exact(NonZeroU16),
}

impl Threads {
    pub fn get(self) -> usize {
        match self {
            Self::All => std::thread::available_parallelism().map_or(1, NonZeroUsize::get),
            Self::Exact(threads) => usize::from(threads.get()),
        }
    }
}

impl From<u16> for Threads {
    fn from(threads: u16) -> Self {
        NonZeroU16::new(threads).map_or(Self::All, Self::Exact)
    }
}

impl std::fmt::Display for Threads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => f.write_str("0"),
            Self::Exact(threads) => write!(f, "{threads}"),
        }
    }
}

impl std::str::FromStr for Threads {
    type Err = std::num::ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse::<u16>().map(Self::from)
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
    pub threads: Threads,
}

pub fn crawl_and_process<O: Op, PM: PageMap + Send + Sync, D: DisplayMode<PM>>(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    op: &O,
    range: &FileRange,
    stats: &Stats,
    display: &D,
    cancellation: &Cancellation,
) -> crate::Result<Vec<O::Output>> {
    cancellation.check()?;
    tracing::info!("starting {} on {} path(s)", O::LABEL, paths.len());
    validate_patterns(crawl_config)?;
    let mut seen_inodes = HashSet::new();
    let mut outputs = Vec::new();
    let result = collect_paths(
        paths,
        crawl_config,
        &mut seen_inodes,
        stats,
        cancellation,
        |path| {
            if let Some(output) =
                display.process_one::<O>(op, &path, range, stats, cancellation)?
            {
                outputs.push(output);
            }
            Ok(())
        },
    );

    display.finish();
    result?;
    cancellation.check()?;
    op.finish()?;
    tracing::info!(
        "done: {} files, {} pages",
        stats.total_files.load(Ordering::Relaxed),
        stats.total_pages.load(Ordering::Relaxed),
    );
    Ok(outputs)
}

pub fn validate_patterns(config: &CrawlConfig) -> crate::Result<()> {
    build_overrides(Path::new("."), config).map(drop)
}

fn collect_paths(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    seen_inodes: &mut HashSet<(u64, u64)>,
    stats: &Stats,
    cancellation: &Cancellation,
    mut emit: impl FnMut(PathBuf) -> crate::Result<()>,
) -> crate::Result<()> {
    cancellation.check()?;
    let mut all_paths: Vec<PathBuf> = paths.to_vec();

    if let Some(batch_path) = &crawl_config.batch {
        let batch_paths = read_batch_paths(batch_path, crawl_config.nul_delim)
            .map_err(|error| crate::Error::io(batch_path.display().to_string(), error))?;
        all_paths.extend(batch_paths);
    }

    let needs_meta = crawl_config.max_file_size.is_some() || !crawl_config.count_hardlinks;

    for path in &all_paths {
        if cancellation.is_cancelled() {
            break;
        }
        let metadata = fs_err::metadata(path)
            .map_err(|error| crate::Error::io(path.display().to_string(), error))?;
        if metadata.is_dir() {
            tracing::info!("crawling directory {}", path.display());
            stats.total_dirs.fetch_add(1, Ordering::Relaxed);
            walk_dir_entries(
                path,
                crawl_config,
                needs_meta,
                seen_inodes,
                cancellation,
                &mut emit,
            )?;
        } else if metadata.is_file() {
            if explicit_file_allowed(
                path,
                &metadata,
                crawl_config,
                needs_meta,
                seen_inodes,
            ) {
                emit(path.clone())?;
            }
        } else {
            return Err(crate::Error::io(
                path.display().to_string(),
                io::Error::new(io::ErrorKind::InvalidInput, "not a file or directory"),
            ));
        }
    }
    Ok(())
}

struct TraversalRoot {
    physical: PathBuf,
    logical: PathBuf,
    device: Option<u64>,
    overrides: Option<Arc<ignore::overrides::Override>>,
    ignored: Option<Arc<ignore::overrides::Override>>,
}

impl TraversalRoot {
    fn new(
        physical: PathBuf,
        logical: PathBuf,
        device: Option<u64>,
        config: &CrawlConfig,
    ) -> crate::Result<Self> {
        Ok(Self {
            physical,
            overrides: build_overrides(&logical, config)?.map(Arc::new),
            ignored: build_ignore_overrides(&logical, config)?.map(Arc::new),
            logical,
            device,
        })
    }

    fn logical_path(&self, physical: &Path) -> PathBuf {
        physical.strip_prefix(&self.physical).map_or_else(
            |_| physical.to_owned(),
            |relative| self.logical.join(relative),
        )
    }
}

fn walk_dir_entries(
    root: &Path,
    config: &CrawlConfig,
    needs_meta: bool,
    seen_inodes: &mut HashSet<(u64, u64)>,
    cancellation: &Cancellation,
    mut emit: impl FnMut(PathBuf) -> crate::Result<()>,
) -> crate::Result<()> {
    use std::os::unix::fs::MetadataExt as _;

    let root_metadata = fs_err::metadata(root)
        .map_err(|error| crate::Error::io(root.display().to_string(), error))?;
    let root_device = config.single_filesystem.then(|| root_metadata.dev());
    let physical_root = if config.follow_symlinks && root.is_symlink() {
        fs_err::canonicalize(root)
            .map_err(|error| crate::Error::io(root.display().to_string(), error))?
    } else {
        root.to_owned()
    };
    let mut pending = VecDeque::from([TraversalRoot::new(
        physical_root.clone(),
        root.to_owned(),
        root_device,
        config,
    )?]);
    let mut visited = HashSet::new();
    if config.follow_symlinks {
        visited.insert(fs_err::canonicalize(&physical_root).unwrap_or(physical_root));
    }

    while let Some(root) = pending.pop_front() {
        if cancellation.is_cancelled() {
            break;
        }

        let root = Arc::new(root);
        let descend_root = Arc::clone(&root);
        let descend_cancellation = cancellation.clone();
        let entries = dua_core::walk(
            &root.physical,
            config.threads.get(),
            Order::Completion,
            move |entry| {
                if descend_cancellation.is_cancelled() {
                    return false;
                }
                if entry.depth == 0 {
                    return true;
                }
                if descend_root.device.is_some_and(|device| {
                    entry
                        .metadata
                        .as_ref()
                        .map_or(true, |metadata| metadata.dev() != device)
                }) {
                    return false;
                }
                let path = descend_root.logical_path(&entry.path());
                !descend_root
                    .ignored
                    .as_ref()
                    .is_some_and(|overrides| overrides.matched(&path, true).is_ignore())
            },
        );

        for entry in entries {
            if cancellation.is_cancelled() {
                break;
            }
            let entry = entry
                .map_err(|error| crate::Error::io(root.logical.display().to_string(), error))?;
            if entry.depth == 0 && entry.file_type.is_dir() {
                continue;
            }

            let physical_path = entry.path();
            let logical_path = root.logical_path(&physical_path);
            if entry.file_type.is_symlink() {
                if !config.follow_symlinks {
                    continue;
                }
                let metadata = fs_err::metadata(&physical_path)
                    .map_err(|error| crate::Error::io(logical_path.display().to_string(), error))?;
                if root.device.is_some_and(|device| metadata.dev() != device) {
                    continue;
                }
                if metadata.is_dir() {
                    let target = fs_err::canonicalize(&physical_path).map_err(|error| {
                        crate::Error::io(logical_path.display().to_string(), error)
                    })?;
                    if visited.insert(target.clone()) {
                        pending.push_back(TraversalRoot::new(
                            target,
                            logical_path,
                            root.device,
                            config,
                        )?);
                    }
                } else if metadata.is_file()
                    && path_allowed(&logical_path, false, config, root.overrides.as_deref())
                    && file_allowed(Some(&metadata), config, needs_meta, seen_inodes)
                {
                    emit(logical_path)?;
                }
                continue;
            }

            if !entry.file_type.is_file()
                || !path_allowed(&logical_path, false, config, root.overrides.as_deref())
                || !file_allowed(
                    entry.metadata.as_ref().ok(),
                    config,
                    needs_meta,
                    seen_inodes,
                )
            {
                continue;
            }
            emit(logical_path)?;
        }
    }
    Ok(())
}

fn explicit_file_allowed(
    path: &Path,
    metadata: &std::fs::Metadata,
    config: &CrawlConfig,
    needs_meta: bool,
    seen_inodes: &mut HashSet<(u64, u64)>,
) -> bool {
    let root = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let overrides =
        build_overrides(root, config).expect("path patterns were validated before traversal");
    if !path_allowed(path, false, config, overrides.as_ref()) {
        return false;
    }
    file_allowed(Some(metadata), config, needs_meta, seen_inodes)
}

fn path_allowed(
    path: &Path,
    is_dir: bool,
    config: &CrawlConfig,
    overrides: Option<&ignore::overrides::Override>,
) -> bool {
    overrides.is_none_or(|overrides| {
        let matched = overrides.matched(path, is_dir);
        !matched.is_ignore() && (config.filter_patterns.is_empty() || matched.is_whitelist())
    })
}

fn file_allowed(
    metadata: Option<&std::fs::Metadata>,
    config: &CrawlConfig,
    needs_meta: bool,
    seen_inodes: &mut HashSet<(u64, u64)>,
) -> bool {
    if needs_meta && metadata.is_none() {
        return false;
    }

    if let Some(max_size) = config.max_file_size
        && let Some(metadata) = metadata
        && metadata.len() > max_size
    {
        return false;
    }

    if !config.count_hardlinks {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Some(metadata) = metadata
                && metadata.nlink() > 1
                && !seen_inodes.insert((metadata.dev(), metadata.ino()))
            {
                return false;
            }
        }
    }

    true
}

fn build_overrides(
    root: &Path,
    config: &CrawlConfig,
) -> crate::Result<Option<ignore::overrides::Override>> {
    if config.ignore_patterns.is_empty() && config.filter_patterns.is_empty() {
        return Ok(None);
    }

    let mut overrides = ignore::overrides::OverrideBuilder::new(root);
    for pattern in &config.ignore_patterns {
        overrides.add(&format!("!{pattern}"))?;
    }
    for pattern in &config.filter_patterns {
        overrides.add(pattern)?;
    }
    Ok(Some(overrides.build()?))
}

fn build_ignore_overrides(
    root: &Path,
    config: &CrawlConfig,
) -> crate::Result<Option<ignore::overrides::Override>> {
    if config.ignore_patterns.is_empty() {
        return Ok(None);
    }

    let mut overrides = ignore::overrides::OverrideBuilder::new(root);
    for pattern in &config.ignore_patterns {
        overrides.add(&format!("!{pattern}"))?;
    }
    Ok(Some(overrides.build()?))
}

pub fn read_batch_paths(path: &Path, nul_delim: bool) -> io::Result<Vec<PathBuf>> {
    use std::os::unix::ffi::OsStrExt;

    let reader: Box<dyn BufRead> = if path == Path::new("-") {
        Box::new(io::stdin().lock())
    } else {
        Box::new(io::BufReader::new(fs_err::File::open(path)?))
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
mod tests {
    use super::*;
    use crate::Cancellation;

    #[test]
    fn threads_parse_and_display() {
        assert_eq!("0".parse::<Threads>().unwrap(), Threads::All);
        assert_eq!("4".parse::<Threads>().unwrap().to_string(), "4");
        assert_eq!(Threads::from(1).get(), 1);
        assert!(Threads::All.get() > 0);
    }

    #[test]
    fn cancelled_collection_emits_no_paths() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let config = CrawlConfig {
            follow_symlinks: false,
            single_filesystem: false,
            count_hardlinks: true,
            ignore_patterns: Vec::new(),
            filter_patterns: Vec::new(),
            max_file_size: None,
            batch: None,
            nul_delim: false,
            threads: Threads::default(),
        };
        let mut seen_inodes = HashSet::new();
        let stats = Stats::new();
        let cancellation = Cancellation::new();
        cancellation.cancel();
        let mut emitted = Vec::new();

        let result = collect_paths(
            &[file.path().to_owned()],
            &config,
            &mut seen_inodes,
            &stats,
            &cancellation,
            |path| {
                emitted.push(path);
                Ok(())
            },
        );

        assert!(emitted.is_empty());
        assert!(matches!(result, Err(crate::Error::Cancelled)));
    }
}
