use pagers_core::output::{Summary, pretty_size};

use crate::cli::OutputFormat;

impl OutputFormat {
    pub fn print_summary(self, summary: &Summary, label: &str, has_action: bool) {
        match self {
            Self::Human => print_human(summary, label, has_action),
            Self::Kv => print_kv(summary, label, has_action),
            Self::Json => print_json(summary, label, has_action),
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

fn print_kv(s: &Summary, label: &str, has_action: bool) {
    let cap = capitalize(label);
    println!("Files={}", s.total_files);
    println!("Directories={}", s.total_dirs);
    if has_action {
        println!("{cap}Pages={}", s.action_pages);
        println!("{cap}Size={}", s.action_size);
        println!("{cap}Percent={:.3}", s.action_pct);
    }
    println!("TotalResidentPages={}", s.total_resident_pages);
    println!("TotalPages={}", s.total_pages);
    println!("TotalResidentSize={}", s.resident_size);
    println!("TotalSize={}", s.total_size);
    println!("TotalResidentPercent={:.3}", s.resident_pct);
    println!("Elapsed={:.5}", s.elapsed);
}

fn print_json(s: &Summary, label: &str, has_action: bool) {
    let mut value = serde_json::json!({
        "files": s.total_files,
        "directories": s.total_dirs,
        "total_resident_pages": s.total_resident_pages,
        "total_pages": s.total_pages,
        "total_resident_size": s.resident_size,
        "total_size": s.total_size,
        "total_resident_percent": s.resident_pct,
        "elapsed": s.elapsed,
    });
    if has_action {
        let obj = value.as_object_mut().unwrap();
        obj.insert(format!("{label}_pages"), s.action_pages.into());
        obj.insert(format!("{label}_size"), s.action_size.into());
        obj.insert(format!("{label}_percent"), s.action_pct.into());
    }
    println!("{value}");
}

fn print_human(s: &Summary, label: &str, has_action: bool) {
    let cap = capitalize(label);
    println!("           Files: {}", s.total_files);
    println!("     Directories: {}", s.total_dirs);
    if has_action {
        print!("  {cap:>8} Pages: {}/{}  ", s.action_pages, s.total_pages);
        print!(
            "{}/{}  ",
            pretty_size(s.action_size),
            pretty_size(s.total_size)
        );
        if s.total_pages > 0 {
            print!("{:.3}%", s.action_pct);
        }
        println!();
    }
    print!(
        "  Resident Pages: {}/{}  ",
        s.total_resident_pages, s.total_pages,
    );
    print!(
        "{}/{}  ",
        pretty_size(s.resident_size),
        pretty_size(s.total_size)
    );
    if s.total_pages > 0 {
        print!("{:.3}%", s.resident_pct);
    }
    println!();
    println!("         Elapsed: {:.5} seconds", s.elapsed);
}
