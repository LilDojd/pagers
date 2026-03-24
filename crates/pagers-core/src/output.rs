use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::ops::Stats;

pub fn pretty_size(bytes: usize) -> String {
    const KI: f64 = 1024.0;
    const MI: f64 = KI * KI;
    const GI: f64 = KI * MI;
    const TI: f64 = KI * GI;

    let b = bytes as f64;
    match b {
        _ if b < KI => format!("{bytes}"),
        _ if b < MI => format!("{:.1}K", b / KI),
        _ if b < GI => format!("{:.1}M", b / MI),
        _ if b < TI => format!("{:.1}G", b / GI),
        _ => format!("{:.1}T", b / TI),
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Summary {
    pub total_files: usize,
    pub total_dirs: usize,
    pub total_pages: usize,
    pub total_resident_pages: usize,
    pub action_pages: usize,
    pub total_size: usize,
    pub resident_size: usize,
    pub action_size: usize,
    pub resident_pct: f64,
    pub action_pct: f64,
    pub elapsed: f64,
}

impl Summary {
    pub fn from_stats(stats: &Stats, elapsed: f64, action_sign: isize) -> Self {
        let page_size = *crate::pagesize::PAGE_SIZE;
        let total_pages = stats.total_pages.load(Ordering::Relaxed);
        let action_pages = stats.action_pages.load(Ordering::Relaxed);
        let initial = stats.initial_pages_in_core.load(Ordering::Relaxed);
        let signed_action = (action_pages as isize) * action_sign;
        let total_resident_pages = initial.saturating_add_signed(signed_action);
        let total_files = stats.total_files.load(Ordering::Relaxed);
        let total_dirs = stats.total_dirs.load(Ordering::Relaxed);

        let total_size = total_pages * page_size;
        let resident_size = total_resident_pages * page_size;
        let action_size = action_pages * page_size;
        let resident_pct = if total_pages > 0 {
            100.0 * total_resident_pages as f64 / total_pages as f64
        } else {
            0.0
        };
        let action_pct = if total_pages > 0 {
            100.0 * action_pages as f64 / total_pages as f64
        } else {
            0.0
        };

        Self {
            total_files,
            total_dirs,
            total_pages,
            total_resident_pages,
            action_pages,
            total_size,
            resident_size,
            action_size,
            resident_pct,
            action_pct,
            elapsed,
        }
    }
}

pub fn pretty_elapsed(secs: f64) -> String {
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        humantime::format_duration(Duration::from_secs(secs as u64)).to_string()
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
        assert_eq!(pretty_size(1024_usize.pow(4)), "1.0T");
    }

    #[test]
    fn test_pretty_elapsed() {
        assert_eq!(pretty_elapsed(0.5), "0.5s");
        assert_eq!(pretty_elapsed(1.0), "1.0s");
        assert_eq!(pretty_elapsed(13.919), "13.9s");
        assert_eq!(pretty_elapsed(59.9), "59.9s");
        assert_eq!(pretty_elapsed(60.0), "1m");
        assert_eq!(pretty_elapsed(61.0), "1m 1s");
        assert_eq!(pretty_elapsed(1801.0), "30m 1s");
        assert_eq!(pretty_elapsed(3661.0), "1h 1m 1s");
    }

    #[test]
    fn test_summary_from_stats_zero() {
        let stats = Stats::new();
        let summary = Summary::from_stats(&stats, 1.0, 0);
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.total_pages, 0);
        assert!((summary.resident_pct - 0.0).abs() < f64::EPSILON);
        assert!((summary.action_pct - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_summary_pct() {
        use std::sync::atomic::Ordering;
        let stats = Stats::new();
        stats.total_pages.store(200, Ordering::Relaxed);
        stats.initial_pages_in_core.store(50, Ordering::Relaxed);
        stats.action_pages.store(100, Ordering::Relaxed);
        // resident = initial(50) + action(100) * sign(1) = 150
        let summary = Summary::from_stats(&stats, 0.5, 1);
        assert!((summary.resident_pct - 75.0).abs() < 0.001);
        assert!((summary.action_pct - 50.0).abs() < 0.001);
    }
}
