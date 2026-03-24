use pagers_core::output::{Summary, pretty_size};

use crate::cli::OutputFormat;

impl OutputFormat {
    pub fn print_summary(self, summary: &Summary, label: &str) {
        match self {
            Self::Human => print_human(summary, label),
            Self::Kv => print_kv(summary, label),
            Self::Json => print_json(summary, label),
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn print_kv(s: &Summary, label: &str) {
    let cap = capitalize(label);
    println!("Files={}", s.total_files);
    println!("Directories={}", s.total_dirs);
    println!("{cap}Pages={}", s.pages_in_core);
    println!("TotalPages={}", s.total_pages);
    println!("{cap}Size={}", s.in_core_size);
    println!("TotalSize={}", s.total_size);
    println!("{cap}Percent={:.3}", s.pct);
    println!("Elapsed={:.5}", s.elapsed);
}

fn print_json(s: &Summary, label: &str) {
    let value = serde_json::json!({
        "files": s.total_files,
        "directories": s.total_dirs,
        format!("{label}_pages"): s.pages_in_core,
        "total_pages": s.total_pages,
        format!("{label}_size"): s.in_core_size,
        "total_size": s.total_size,
        format!("{label}_percent"): s.pct,
        "elapsed": s.elapsed,
    });
    println!("{value}");
}

fn print_human(s: &Summary, label: &str) {
    let cap = capitalize(label);
    println!("           Files: {}", s.total_files);
    println!("     Directories: {}", s.total_dirs);
    match label {
        "resident" => {
            print!("  Resident Pages: {}/{}  ", s.pages_in_core, s.total_pages,);
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
