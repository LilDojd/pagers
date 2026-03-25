use pagers_core::output::{Summary, pretty_elapsed, pretty_size};

pub struct Human;
pub struct Kv;
pub struct Json;

pub trait OutputFormat<F> {
    fn print(&self, label: &str, has_action: bool);
}

impl crate::cli::OutputFormatArg {
    pub fn print_summary(
        self,
        summary: &pagers_core::output::Summary,
        label: &str,
        has_action: bool,
    ) {
        use crate::output::{Human, Json, Kv, OutputFormat};
        match self {
            Self::Human => OutputFormat::<Human>::print(summary, label, has_action),
            Self::Kv => OutputFormat::<Kv>::print(summary, label, has_action),
            Self::Json => OutputFormat::<Json>::print(summary, label, has_action),
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

impl OutputFormat<Human> for Summary {
    fn print(&self, label: &str, has_action: bool) {
        let cap = capitalize(label);
        println!("           Files: {}", self.total_files);
        println!("     Directories: {}", self.total_dirs);
        if has_action {
            print!(
                "  {cap:>8} Pages: {}/{}  ",
                self.action_pages, self.total_pages
            );
            print!(
                "{}/{}  ",
                pretty_size(self.action_size),
                pretty_size(self.total_size)
            );
            if self.total_pages > 0 {
                print!("{:.3}%", self.action_pct);
            }
            println!();
        }
        print!(
            "  Resident Pages: {}/{}  ",
            self.total_resident_pages, self.total_pages,
        );
        print!(
            "{}/{}  ",
            pretty_size(self.resident_size),
            pretty_size(self.total_size)
        );
        if self.total_pages > 0 {
            print!("{:.3}%", self.resident_pct);
        }
        println!();
        println!("         Elapsed: {}", pretty_elapsed(self.elapsed));
    }
}

impl OutputFormat<Kv> for Summary {
    fn print(&self, label: &str, has_action: bool) {
        let cap = capitalize(label);
        println!("Files={}", self.total_files);
        println!("Directories={}", self.total_dirs);
        if has_action {
            println!("{cap}Pages={}", self.action_pages);
            println!("{cap}Size={}", self.action_size);
            println!("{cap}Percent={:.3}", self.action_pct);
        }
        println!("TotalResidentPages={}", self.total_resident_pages);
        println!("TotalPages={}", self.total_pages);
        println!("TotalResidentSize={}", self.resident_size);
        println!("TotalSize={}", self.total_size);
        println!("TotalResidentPercent={:.3}", self.resident_pct);
        println!("Elapsed={:.5}", self.elapsed);
    }
}

impl OutputFormat<Json> for Summary {
    fn print(&self, label: &str, has_action: bool) {
        let mut value = serde_json::json!({
            "files": self.total_files,
            "directories": self.total_dirs,
            "total_resident_pages": self.total_resident_pages,
            "total_pages": self.total_pages,
            "total_resident_size": self.resident_size,
            "total_size": self.total_size,
            "total_resident_percent": self.resident_pct,
            "elapsed": self.elapsed,
        });
        if has_action {
            let obj = value
                .as_object_mut()
                .expect("json! macro always produces an object");
            obj.insert(format!("{label}_pages"), self.action_pages.into());
            obj.insert(format!("{label}_size"), self.action_size.into());
            obj.insert(format!("{label}_percent"), self.action_pct.into());
        }
        println!("{value}");
    }
}
