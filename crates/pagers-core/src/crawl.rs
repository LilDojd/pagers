//! Directory traversal with inode dedup and filtering.

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use dashmap::DashMap;
use ignore::WalkBuilder;

use crate::events::Event;
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

/// Crawl paths and process files. Returns op outputs (non-empty files only).
pub fn crawl_and_process<O: Op>(
    paths: &[PathBuf],
    crawl_config: &CrawlConfig,
    op: &O,
    range: &FileRange,
    stats: &Stats,
    events: Option<&Sender<Event>>,
) -> Vec<O::Output> {
    let seen_inodes: Arc<DashMap<(u64, u64), ()>> = Arc::new(DashMap::new());
    let outputs: Arc<std::sync::Mutex<Vec<O::Output>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut all_paths: Vec<PathBuf> = paths.to_vec();

    if let Some(batch_path) = &crawl_config.batch {
        match read_batch_paths(batch_path, crawl_config.nul_delim) {
            Ok(batch_paths) => all_paths.extend(batch_paths),
            Err(e) => eprintln!("pagers: WARNING: batch file: {e}"),
        }
    }

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
                        eprintln!("pagers: WARNING: {e}");
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

                if let Some(max_size) = crawl_config.max_file_size
                    && let Ok(meta) = entry_path.metadata()
                    && meta.len() > max_size
                {
                    continue;
                }

                if !crawl_config.count_hardlinks {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if let Ok(meta) = entry_path.metadata()
                            && meta.nlink() > 1
                        {
                            let key = (meta.dev(), meta.ino());
                            if seen_inodes.contains_key(&key) {
                                continue;
                            }
                            seen_inodes.insert(key, ());
                        }
                    }
                }

                process_entry(entry_path, op, range, stats, events, &outputs);
            }
        } else if path.is_file() {
            process_entry(path, op, range, stats, events, &outputs);
        } else {
            eprintln!(
                "pagers: WARNING: skipping {}: not a file or directory",
                path.display()
            );
        }
    }

    match Arc::try_unwrap(outputs) {
        Ok(mutex) => mutex.into_inner().expect("mutex poisoned"),
        Err(_) => panic!("Arc still has multiple owners"),
    }
}

fn process_entry<O: Op>(
    path: &Path,
    op: &O,
    range: &FileRange,
    stats: &Stats,
    events: Option<&Sender<Event>>,
    outputs: &Arc<std::sync::Mutex<Vec<O::Output>>>,
) {
    match ops::process_file(op, path, range, stats, events) {
        Ok(Some(output)) => {
            outputs.lock().unwrap().push(output);
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("pagers: WARNING: {}: {e}", path.display());
        }
    }
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
