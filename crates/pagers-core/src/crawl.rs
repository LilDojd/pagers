use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

use ignore::WalkBuilder;

use crate::events::{Event, EventSink};
use crate::mincore::PageMap;
use crate::ops::{self, FileRange, Op, Stats};
use crate::par::{InodeSet, SeenInodes as _, par_collect};

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
}

pub fn crawl_and_process<O: Op, PM: PageMap + Send>(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    op: &O,
    range: &FileRange,
    stats: &Stats,
    events: Option<Sender<Event<PM>>>,
) -> crate::Result<Vec<O::Output>> {
    let seen_inodes = InodeSet::default();
    let file_paths = collect_file_paths(paths, crawl_config, &seen_inodes, stats);

    let sink = events.map(EventSink::new);
    let sink_ref = sink.as_ref();
    let need_residency = sink_ref.is_some();

    let process_one = |path: &PathBuf| -> Option<O::Output> {
        let path_str = path.display().to_string();

        if let Some(sink) = sink_ref {
            let full_file = FileRange {
                offset: 0,
                max_len: None,
            };
            match ops::file_info::<PM>(path, &full_file) {
                Ok(Some(info)) => sink.send(Event::FileStart {
                    path: path_str.clone(),
                    total_pages: info.total_pages,
                    residency: info.residency,
                }),
                Ok(None) => return None,
                Err(e) => {
                    tracing::warn!("{}: {e}", path.display());
                    return None;
                }
            }
        }

        let page_offset = range.offset as usize / *crate::pagesize::PAGE_SIZE;
        let on_progress;
        let on_progress_ref = if let Some(sink) = sink_ref {
            on_progress = move |pages_walked: usize| {
                sink.send(Event::FileProgress {
                    path: path_str.clone(),
                    page_offset,
                    pages_walked,
                });
            };
            Some(&on_progress as &(dyn Fn(usize) + Sync))
        } else {
            None
        };

        let result =
            match ops::process_file::<O, PM>(op, path, range, need_residency, on_progress_ref) {
                Ok(Some(r)) => r,
                Ok(None) => return None,
                Err(e) => {
                    tracing::warn!("{e}");
                    return None;
                }
            };

        stats
            .total_pages
            .fetch_add(result.total_pages as i64, Ordering::Relaxed);
        stats.total_files.fetch_add(1, Ordering::Relaxed);
        stats
            .total_pages_in_core
            .fetch_add(result.pages_in_core_after, Ordering::Relaxed);

        if let Some(sink) = sink_ref {
            let full_file = FileRange {
                offset: 0,
                max_len: None,
            };
            if let Ok(Some(info)) = ops::file_info::<PM>(path, &full_file) {
                sink.send(Event::FileDone {
                    path: path.display().to_string(),
                    pages_in_core: info.residency.count_filled(),
                    total_pages: info.total_pages,
                    residency: info.residency,
                });
            }
        }

        Some(result.output)
    };

    let outputs = par_collect(&file_paths, process_one);

    if let Some(sink) = sink {
        sink.send(Event::AllDone);
    }

    op.finish()?;

    Ok(outputs)
}

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
            collect_dir_entries(path, crawl_config, needs_meta, seen_inodes, &mut file_paths);
        } else if path.is_file() {
            file_paths.push(path.clone());
        } else {
            tracing::warn!("skipping {}: not a file or directory", path.display());
        }
    }

    file_paths
}

fn collect_dir_entries(
    root: &Path,
    config: &CrawlConfig,
    needs_meta: bool,
    seen_inodes: &InodeSet,
    out: &mut Vec<PathBuf>,
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

        out.push(entry_path.to_path_buf());
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
