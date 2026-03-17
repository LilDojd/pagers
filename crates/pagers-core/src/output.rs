//! Human-readable and machine-readable output formatting.

use std::sync::atomic::Ordering;

use crate::mmap;
use crate::ops::Stats;

pub fn pretty_size(bytes: i64) -> String {
    if bytes < 1024 {
        return format!("{bytes}");
    }
    let kb = bytes / 1024;
    if kb < 1024 {
        return format!("{kb}K");
    }
    let mb = kb / 1024;
    if mb < 1024 {
        return format!("{mb}M");
    }
    let gb = mb / 1024;
    format!("{gb}G")
}

pub fn print_summary(stats: &Stats, elapsed: f64, mode: &str, output_format: Option<&str>) {
    let page_size = mmap::page_size() as i64;
    let total_pages = stats.total_pages.load(Ordering::Relaxed);
    let pages_in_core = stats.total_pages_in_core.load(Ordering::Relaxed);
    let total_files = stats.total_files.load(Ordering::Relaxed);
    let total_dirs = stats.total_dirs.load(Ordering::Relaxed);

    let total_size = total_pages * page_size;
    let in_core_size = pages_in_core * page_size;
    let pct = if total_pages > 0 {
        100.0 * pages_in_core as f64 / total_pages as f64
    } else {
        0.0
    };

    match output_format {
        Some("kv") => {
            let desc = match mode {
                "touch" => "Touched",
                "evict" => "Evicted",
                _ => "Resident",
            };
            println!(
                "Files={total_files} Directories={total_dirs} \
                 {desc}Pages={pages_in_core} TotalPages={total_pages} \
                 {desc}Size={in_core_size} TotalSize={total_size} \
                 {desc}Percent={pct:.3} Elapsed={elapsed:.5}"
            );
        }
        _ => {
            println!("           Files: {total_files}");
            println!("     Directories: {total_dirs}");
            match mode {
                "touch" => println!(
                    "   Touched Pages: {total_pages} ({})",
                    pretty_size(total_size)
                ),
                "evict" => println!(
                    "   Evicted Pages: {total_pages} ({})",
                    pretty_size(total_size)
                ),
                _ => {
                    print!("  Resident Pages: {pages_in_core}/{total_pages}  ",);
                    print!(
                        "{}/{}  ",
                        pretty_size(in_core_size),
                        pretty_size(total_size)
                    );
                    if total_pages > 0 {
                        print!("{pct:.3}%");
                    }
                    println!();
                }
            }
            println!("         Elapsed: {elapsed:.5} seconds");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pretty_size() {
        assert_eq!(pretty_size(0), "0");
        assert_eq!(pretty_size(512), "512");
        assert_eq!(pretty_size(1024), "1K");
        assert_eq!(pretty_size(1024 * 1024), "1M");
        assert_eq!(pretty_size(1024 * 1024 * 1024), "1G");
        assert_eq!(pretty_size(2 * 1024 * 1024 * 1024), "2G");
    }
}
