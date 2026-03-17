use std::collections::HashMap;

use pagers_core::events::Event as CoreEvent;

use crate::event::TuiEvent;
use crate::state::FileState;

#[derive(Default)]
pub struct App {
    files: Vec<FileState>,
    file_index: HashMap<String, usize>,
}

pub enum ControlFlow {
    Continue,
    Quit,
    Done,
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn handle_event(&mut self, event: TuiEvent) -> ControlFlow {
        match event {
            TuiEvent::Core(CoreEvent::FileStart {
                path,
                total_pages,
                residency,
            }) => {
                let pages_in_core = residency.iter().filter(|&&b| b).count();
                let idx = self.files.len();
                self.file_index.insert(path.clone(), idx);
                self.files.push(FileState {
                    path,
                    total_pages,
                    pages_in_core,
                    residency,
                    done: false,
                });
                ControlFlow::Continue
            }
            TuiEvent::Core(CoreEvent::FileProgress { path, residency }) => {
                let pages_in_core = residency.iter().filter(|&&b| b).count();
                if let Some(&idx) = self.file_index.get(&path) {
                    self.files[idx].pages_in_core = pages_in_core;
                    self.files[idx].residency = residency;
                }
                ControlFlow::Continue
            }
            TuiEvent::Core(CoreEvent::FileDone {
                path,
                pages_in_core,
                residency,
                ..
            }) => {
                if let Some(&idx) = self.file_index.get(&path) {
                    self.files[idx].pages_in_core = pages_in_core;
                    self.files[idx].residency = residency;
                    self.files[idx].done = true;
                }
                ControlFlow::Continue
            }
            TuiEvent::CoreDone => ControlFlow::Done,
            TuiEvent::Quit => ControlFlow::Quit,
        }
    }

    pub fn files(&self) -> Vec<&FileState> {
        self.files.iter().collect()
    }

    /// Return files for the live TUI viewport: sorted by size descending
    /// (path tiebreaker), with done files hidden when total exceeds `max`.
    pub fn visible_files(&self, max: usize) -> Vec<&FileState> {
        let mut files: Vec<&FileState> = self.files.iter().collect();
        files.sort_by(|a, b| {
            b.total_pages
                .cmp(&a.total_pages)
                .then_with(|| a.path.cmp(&b.path))
        });
        if files.len() > max {
            files.retain(|f| !f.done);
        }
        files.truncate(max);
        files
    }

    pub fn into_files(self) -> Vec<FileState> {
        let mut files = self.files;
        files.sort_by(|a, b| a.ratio().total_cmp(&b.ratio()));
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pagers_core::events::Event as CoreEvent;

    #[test]
    fn test_handle_file_start() {
        let mut app = App::new();
        let flow = app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/test.bin".to_string(),
            total_pages: 100,
            residency: vec![true; 50],
        }));
        assert!(matches!(flow, ControlFlow::Continue));
        assert_eq!(app.files().len(), 1);
        assert_eq!(app.files()[0].pages_in_core, 50);
    }

    #[test]
    fn test_handle_file_progress_uses_hashmap() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/a.bin".to_string(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileProgress {
            path: "/a.bin".to_string(),
            residency: vec![true; 100],
        }));
        assert_eq!(app.files()[0].pages_in_core, 100);
    }

    #[test]
    fn test_handle_file_done_sets_flag() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/a.bin".to_string(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone {
            path: "/a.bin".to_string(),
            pages_in_core: 100,
            total_pages: 100,
            residency: vec![true; 100],
        }));
        assert!(app.files()[0].done);
    }

    #[test]
    fn test_files_in_insertion_order() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/first.bin".to_string(),
            total_pages: 100,
            residency: vec![true; 90],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/second.bin".to_string(),
            total_pages: 100,
            residency: vec![true; 10],
        }));
        let files = app.files();
        assert_eq!(files[0].path, "/first.bin");
        assert_eq!(files[1].path, "/second.bin");
    }

    #[test]
    fn test_visible_files_sorted_by_size_desc() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/small.bin".to_string(),
            total_pages: 10,
            residency: vec![false; 10],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/big.bin".to_string(),
            total_pages: 1000,
            residency: vec![false; 1000],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/mid.bin".to_string(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        let vis = app.visible_files(8);
        assert_eq!(vis[0].path, "/big.bin");
        assert_eq!(vis[1].path, "/mid.bin");
        assert_eq!(vis[2].path, "/small.bin");
    }

    #[test]
    fn test_visible_files_hides_done_when_overflow() {
        let mut app = App::new();
        // Add 3 files, mark one done, max=2
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/a.bin".to_string(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/b.bin".to_string(),
            total_pages: 200,
            residency: vec![false; 200],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/c.bin".to_string(),
            total_pages: 50,
            residency: vec![false; 50],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone {
            path: "/b.bin".to_string(),
            pages_in_core: 200,
            total_pages: 200,
            residency: vec![true; 200],
        }));
        let vis = app.visible_files(2);
        assert_eq!(vis.len(), 2);
        // /b.bin is done and should be hidden since 3 > 2
        assert!(vis.iter().all(|f| f.path != "/b.bin"));
    }

    #[test]
    fn test_visible_files_keeps_done_when_fits() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/a.bin".to_string(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone {
            path: "/a.bin".to_string(),
            pages_in_core: 100,
            total_pages: 100,
            residency: vec![true; 100],
        }));
        // Only 1 file, max=8 → done file stays visible
        let vis = app.visible_files(8);
        assert_eq!(vis.len(), 1);
        assert!(vis[0].done);
    }

    #[test]
    fn test_core_done_returns_done() {
        let mut app = App::new();
        let flow = app.handle_event(TuiEvent::CoreDone);
        assert!(matches!(flow, ControlFlow::Done));
    }

    #[test]
    fn test_quit_returns_quit() {
        let mut app = App::new();
        let flow = app.handle_event(TuiEvent::Quit);
        assert!(matches!(flow, ControlFlow::Quit));
    }
}
