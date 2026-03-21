//! Summary output formatting.

use std::sync::atomic::Ordering;

use crate::mmap;
use crate::ops::Stats;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Kv,
    Json,
}

impl OutputFormat {
    pub fn print_summary(self, summary: &Summary, label: &str) {
        match self {
            Self::Human => print_human(summary, label),
            Self::Kv => print_kv(summary, label),
            Self::Json => print_json(summary, label),
        }
    }
}

pub fn pretty_size(bytes: i64) -> String {
    const KI: f64 = 1024.0;
    const MI: f64 = KI * KI;
    const GI: f64 = KI * MI;
    const TI: f64 = KI * GI;

    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes}")
    } else if b < MI {
        format!("{:.1}K", b / KI)
    } else if b < GI {
        format!("{:.1}M", b / MI)
    } else if b < TI {
        format!("{:.1}G", b / GI)
    } else {
        format!("{:.1}T", b / TI)
    }
}

pub struct Summary {
    pub total_files: i64,
    pub total_dirs: i64,
    pub total_pages: i64,
    pub pages_in_core: i64,
    pub total_size: i64,
    pub in_core_size: i64,
    pub pct: f64,
    pub elapsed: f64,
}

impl Summary {
    pub fn from_stats(stats: &Stats, elapsed: f64) -> Self {
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

        Self {
            total_files,
            total_dirs,
            total_pages,
            pages_in_core,
            total_size,
            in_core_size,
            pct,
            elapsed,
        }
    }
}

// ── Formatters ──────────────────────────────────────────────────────────────

/// Capitalize the first character of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn print_kv(s: &Summary, label: &str) {
    let cap = capitalize(label);
    println!(
        "Files={} Directories={} \
         {cap}Pages={} TotalPages={} \
         {cap}Size={} TotalSize={} \
         {cap}Percent={:.3} Elapsed={:.5}",
        s.total_files,
        s.total_dirs,
        s.pages_in_core,
        s.total_pages,
        s.in_core_size,
        s.total_size,
        s.pct,
        s.elapsed,
    );
}

fn print_json(s: &Summary, label: &str) {
    let mut map = serde_json::Map::new();
    map.insert("files".into(), s.total_files.into());
    map.insert("directories".into(), s.total_dirs.into());
    map.insert(format!("{label}_pages"), s.pages_in_core.into());
    map.insert("total_pages".into(), s.total_pages.into());
    map.insert(format!("{label}_size"), s.in_core_size.into());
    map.insert("total_size".into(), s.total_size.into());
    map.insert(
        format!("{label}_percent"),
        serde_json::Number::from_f64(s.pct)
            .unwrap_or_else(|| serde_json::Number::from(0))
            .into(),
    );
    map.insert(
        "elapsed".into(),
        serde_json::Number::from_f64(s.elapsed)
            .unwrap_or_else(|| serde_json::Number::from(0))
            .into(),
    );

    let value = serde_json::Value::Object(map);
    println!("{value}");
}

fn print_human(s: &Summary, label: &str) {
    let cap = capitalize(label);
    println!("           Files: {}", s.total_files);
    println!("     Directories: {}", s.total_dirs);
    match label {
        "resident" => {
            print!(
                "  Resident Pages: {}/{}  ",
                s.pages_in_core, s.total_pages,
            );
            print!(
                "{}/{}  ",
                pretty_size(s.in_core_size),
                pretty_size(s.total_size)
            );
            if s.total_pages > 0 {
                print!("{:.3}%", s.pct);
            }
            println!();
        }
        _ => {
            println!(
                "  {cap:>8} Pages: {} ({})",
                s.total_pages,
                pretty_size(s.total_size)
            );
        }
    }
    println!("         Elapsed: {:.5} seconds", s.elapsed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pretty_size() {
        assert_eq!(pretty_size(0), "0");
        assert_eq!(pretty_size(512), "512");
        assert_eq!(pretty_size(1024), "1.0K");
        assert_eq!(pretty_size(1536), "1.5K");
        assert_eq!(pretty_size(1024 * 1024), "1.0M");
        assert_eq!(pretty_size(1024 * 1024 * 1024), "1.0G");
        assert_eq!(pretty_size(2 * 1024 * 1024 * 1024), "2.0G");
        assert_eq!(pretty_size(1024_i64.pow(4)), "1.0T");
    }

    #[test]
    fn test_pretty_size_negative() {
        assert_eq!(pretty_size(-100), "-100");
    }

    #[test]
    fn test_summary_from_stats_zero() {
        let stats = Stats::new();
        let summary = Summary::from_stats(&stats, 1.0);
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.total_pages, 0);
        assert!((summary.pct - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_summary_pct_calculation() {
        use std::sync::atomic::Ordering;
        let stats = Stats::new();
        stats.total_pages.store(200, Ordering::Relaxed);
        stats.total_pages_in_core.store(100, Ordering::Relaxed);
        let summary = Summary::from_stats(&stats, 0.5);
        assert!((summary.pct - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("touched"), "Touched");
        assert_eq!(capitalize("resident"), "Resident");
        assert_eq!(capitalize(""), "");
    }
}
