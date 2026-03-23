use std::sync::atomic::Ordering;

use crate::mmap;
use crate::ops::Stats;

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
}
