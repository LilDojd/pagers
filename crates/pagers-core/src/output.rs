//! Human-readable and machine-readable output formatting.

use std::fmt;
use std::sync::atomic::Ordering;

use crate::mmap;
use crate::ops::Stats;

/// Operation mode, used to label summary output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Query,
    Touch,
    Evict,
    Lock,
    Lockall,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Query => "query",
            Self::Touch => "touch",
            Self::Evict => "evict",
            Self::Lock => "lock",
            Self::Lockall => "lockall",
        })
    }
}

/// Output format for summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Pretty,
    Kv,
    Kv,
    Json,
}

pub fn pretty_size(bytes: i64) -> String {
    const KI: f64 = 1024.0;
    const MI: f64 = 1024.0 * 1024.0;
    const GI: f64 = 1024.0 * 1024.0 * 1024.0;
    const TI: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;

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

/// Collect summary data from stats into a structured form.
pub struct Summary {
    pub total_files: i64,
    pub total_dirs: i64,
    pub total_pages: i64,
    pub pages_in_core: i64,
    pub total_size: i64,
    pub in_core_size: i64,
    pub pct: f64,
    pub elapsed: f64,
    pub mode: Mode,
}

impl Summary {
    pub fn from_stats(stats: &Stats, elapsed: f64, mode: Mode) -> Self {
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
            mode,
        }
    }

    pub fn print(&self, format: OutputFormat) {
        match format {
            OutputFormat::Kv => self.print_kv(),
            OutputFormat::Json => self.print_json(),
            OutputFormat::Pretty => self.print_pretty(),
        }
    }

    fn print_kv(&self) {
        let desc = match self.mode {
            Mode::Touch => "Touched",
            Mode::Evict => "Evicted",
            _ => "Resident",
        };
        println!(
            "Files={} Directories={} \
             {desc}Pages={} TotalPages={} \
             {desc}Size={} TotalSize={} \
             {desc}Percent={:.3} Elapsed={:.5}",
            self.total_files,
            self.total_dirs,
            self.pages_in_core,
            self.total_pages,
            self.in_core_size,
            self.total_size,
            self.pct,
            self.elapsed,
        );
    }

    fn print_json(&self) {
        let desc = match self.mode {
            Mode::Touch => "touched",
            Mode::Evict => "evicted",
            _ => "resident",
        };
        // Manual JSON to avoid adding serde dependency
        println!(
            "{{\
            \"files\":{},\
            \"directories\":{},\
            \"{desc}_pages\":{},\
            \"total_pages\":{},\
            \"{desc}_size\":{},\
            \"total_size\":{},\
            \"{desc}_percent\":{:.3},\
            \"elapsed\":{:.5}\
            }}",
            self.total_files,
            self.total_dirs,
            self.pages_in_core,
            self.total_pages,
            self.in_core_size,
            self.total_size,
            self.pct,
            self.elapsed,
        );
    }

    fn print_pretty(&self) {
        println!("           Files: {}", self.total_files);
        println!("     Directories: {}", self.total_dirs);
        match self.mode {
            Mode::Touch => println!(
                "   Touched Pages: {} ({})",
                self.total_pages,
                pretty_size(self.total_size)
            ),
            Mode::Evict => println!(
                "   Evicted Pages: {} ({})",
                self.total_pages,
                pretty_size(self.total_size)
            ),
            _ => {
                print!(
                    "  Resident Pages: {}/{}  ",
                    self.pages_in_core, self.total_pages,
                );
                print!(
                    "{}/{}  ",
                    pretty_size(self.in_core_size),
                    pretty_size(self.total_size)
                );
                if self.total_pages > 0 {
                    print!("{:.3}%", self.pct);
                }
                println!();
            }
        }
        println!("         Elapsed: {:.5} seconds", self.elapsed);
    }
}

/// Print summary in the given format. Convenience wrapper.
pub fn print_summary(stats: &Stats, elapsed: f64, mode: Mode, format: OutputFormat) {
    Summary::from_stats(stats, elapsed, mode).print(format);
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
        let summary = Summary::from_stats(&stats, 1.0, Mode::Query);
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.total_dirs, 0);
        assert_eq!(summary.total_pages, 0);
        assert_eq!(summary.pages_in_core, 0);
        assert!((summary.pct - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_summary_pct_calculation() {
        use std::sync::atomic::Ordering;
        let stats = Stats::new();
        stats.total_pages.store(200, Ordering::Relaxed);
        stats.total_pages_in_core.store(100, Ordering::Relaxed);
        let summary = Summary::from_stats(&stats, 0.5, Mode::Query);
        assert!((summary.pct - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_mode_display() {
        assert_eq!(Mode::Query.to_string(), "query");
        assert_eq!(Mode::Touch.to_string(), "touch");
        assert_eq!(Mode::Evict.to_string(), "evict");
        assert_eq!(Mode::Lock.to_string(), "lock");
        assert_eq!(Mode::Lockall.to_string(), "lockall");
    }
}
