use pagers_core::output::{Summary, pretty_elapsed, pretty_size};

impl crate::cli::OutputFormatArg {
    pub fn print_summary(
        self,
        summary: &pagers_core::output::Summary,
        label: &str,
        has_action: bool,
    ) {
        match self {
            Self::Human => print_human(summary, label, has_action),
            Self::Kv => print_kv(summary, label, has_action),
            Self::Json => print_json(summary, label, has_action),
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut cap = s.to_string();
    if let Some(c) = cap.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    cap
}

fn print_human(summary: &Summary, label: &str, has_action: bool) {
    let cap = capitalize(label);
    println!("           Files: {}", summary.total_files);
    println!("     Directories: {}", summary.total_dirs);
    if has_action {
        print!(
            "  {cap:>8} Pages: {}/{}  ",
            summary.action_pages, summary.total_pages
        );
        print!(
            "{}/{}  ",
            pretty_size(summary.action_size),
            pretty_size(summary.total_size)
        );
        if summary.total_pages > 0 {
            print!("{:.3}%", summary.action_pct);
        }
        println!();
    }
    print!(
        "  Resident Pages: {}/{}  ",
        summary.total_resident_pages, summary.total_pages,
    );
    print!(
        "{}/{}  ",
        pretty_size(summary.resident_size),
        pretty_size(summary.total_size)
    );
    if summary.total_pages > 0 {
        print!("{:.3}%", summary.resident_pct);
    }
    println!();
    println!("         Elapsed: {}", pretty_elapsed(summary.elapsed));
}

fn print_kv(summary: &Summary, label: &str, has_action: bool) {
    let cap = capitalize(label);
    println!("Files={}", summary.total_files);
    println!("Directories={}", summary.total_dirs);
    if has_action {
        println!("{cap}Pages={}", summary.action_pages);
        println!("{cap}Size={}", summary.action_size);
        println!("{cap}Percent={:.3}", summary.action_pct);
    }
    println!("TotalResidentPages={}", summary.total_resident_pages);
    println!("TotalPages={}", summary.total_pages);
    println!("TotalResidentSize={}", summary.resident_size);
    println!("TotalSize={}", summary.total_size);
    println!("TotalResidentPercent={:.3}", summary.resident_pct);
    println!("Elapsed={:.5}", summary.elapsed);
}

fn print_json(summary: &Summary, label: &str, has_action: bool) {
    let mut value = serde_json::json!({
        "files": summary.total_files,
        "directories": summary.total_dirs,
        "total_resident_pages": summary.total_resident_pages,
        "total_pages": summary.total_pages,
        "total_resident_size": summary.resident_size,
        "total_size": summary.total_size,
        "total_resident_percent": summary.resident_pct,
        "elapsed": summary.elapsed,
    });
    if has_action {
        let obj = value
            .as_object_mut()
            .expect("json! macro always produces an object");
        obj.insert(format!("{label}_pages"), summary.action_pages.into());
        obj.insert(format!("{label}_size"), summary.action_size.into());
        obj.insert(format!("{label}_percent"), summary.action_pct.into());
    }
    println!("{value}");
}
