//! Directory traversal with inode dedup and filtering.

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use dashmap::DashMap;
use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::events::{Event, EventSink};
use crate::ops::{self, FileRange, Op, Stats};

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

pub fn crawl_and_process<O: Op>(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    op: &O,
    range: &FileRange,
    stats: &Stats,
    events: Option<Sender<Event>>,
) -> crate::Result<Vec<O::Output>> {
    let seen_inodes: DashMap<(u64, u64), ()> = DashMap::new();
    let file_paths = collect_file_paths(paths, crawl_config, &seen_inodes, stats);

    let sink = events.map(EventSink::new);
    let sink_ref = sink.as_ref();

    // Discovery phase: send FileStart for all files so the TUI sees them upfront.
    let discovered = if let Some(sink) = sink_ref {
        file_paths.par_iter().for_each(|path| {
            if let Err(e) = ops::send_file_start(path, range, sink) {
                tracing::warn!("{}: {e}", path.display());
            }
        });
        true
    } else {
        false
    };

    let outputs: Vec<O::Output> = file_paths
        .par_iter()
        .filter_map(
            |path| match ops::process_file(op, path, range, stats, sink_ref, discovered) {
                Ok(Some(output)) => Some(output),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!("{e}");
                    None
                }
            },
        )
        .collect();

    if let Some(ref sink) = sink {
        sink.send(Event::AllDone);
    }

    op.finish()?;

    Ok(outputs)
}

fn collect_file_paths(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    seen_inodes: &DashMap<(u64, u64), ()>,
    stats: &Stats,
) -> Vec<PathBuf> {
    let mut all_paths: Vec<PathBuf> = paths.to_vec();

    if let Some(batch_path) = &crawl_config.batch {
        match read_batch_paths(batch_path, crawl_config.nul_delim) {
            Ok(batch_paths) => all_paths.extend(batch_paths),
            Err(e) => tracing::warn!("batch file: {e}"),
        }
    }

    let mut file_paths = Vec::new();

    for path in &all_paths {
        if path.is_dir() {
            use std::sync::atomic::Ordering;
            stats.total_dirs.fetch_add(1, Ordering::Relaxed);

            let mut builder = WalkBuilder::new(path);
            builder
                .follow_links(crawl_config.follow_symlinks)
                .same_file_system(crawl_config.single_filesystem)
                .hidden(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false);

            if !crawl_config.ignore_patterns.is_empty() || !crawl_config.filter_patterns.is_empty()
            {
                let mut overrides = ignore::overrides::OverrideBuilder::new(path);
                for pat in &crawl_config.ignore_patterns {
                    let _ = overrides.add(&format!("!{pat}"));
                }
                for pat in &crawl_config.filter_patterns {
                    let _ = overrides.add(pat);
                }
                if let Ok(ov) = overrides.build() {
                    builder.overrides(ov);
                }
            }

            for entry in builder.build() {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("{e}");
                        continue;
                    }
                };

                let ft = match entry.file_type() {
                    Some(ft) => ft,
                    None => continue,
                };

                if ft.is_dir() || !ft.is_file() {
                    continue;
                }

                let entry_path = entry.path();

                let needs_meta =
                    crawl_config.max_file_size.is_some() || !crawl_config.count_hardlinks;
                let meta = if needs_meta {
                    entry_path.metadata().ok()
                } else {
                    None
                };

                if let Some(max_size) = crawl_config.max_file_size
                    && let Some(ref m) = meta
                    && m.len() > max_size
                {
                    continue;
                }

                if !crawl_config.count_hardlinks {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if let Some(ref m) = meta
                            && m.nlink() > 1
                        {
                            let key = (m.dev(), m.ino());
                            if seen_inodes.contains_key(&key) {
                                continue;
                            }
                            seen_inodes.insert(key, ());
                        }
                    }
                }

                file_paths.push(entry_path.to_path_buf());
            }
        } else if path.is_file() {
            file_paths.push(path.clone());
        } else {
            tracing::warn!("skipping {}: not a file or directory", path.display());
        }
    }

    file_paths
}

fn read_batch_paths(path: &Path, nul_delim: bool) -> io::Result<Vec<PathBuf>> {
    let mut reader: Box<dyn BufRead> = if path == Path::new("-") {
        Box::new(io::stdin().lock())
    } else {
        Box::new(io::BufReader::new(std::fs::File::open(path)?))
    };

    let mut paths = Vec::new();

    if nul_delim {
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let n = reader.read_until(0, &mut buf)?;
            if n == 0 {
                break;
            }
            if buf.last() == Some(&0) {
                buf.pop();
            }
            if !buf.is_empty() {
                paths.push(PathBuf::from(String::from_utf8_lossy(&buf).into_owned()));
            }
        }
    } else {
        for line in reader.lines() {
            let line = line?;
            if !line.is_empty() {
                paths.push(PathBuf::from(line));
            }
        }
    }

    Ok(paths)
}
