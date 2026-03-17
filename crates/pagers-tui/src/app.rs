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
                ..
            }) => {
                if let Some(&idx) = self.file_index.get(&path) {
                    self.files[idx].pages_in_core = pages_in_core;
                    self.files[idx].done = true;
                }
                ControlFlow::Continue
            }
            TuiEvent::CoreDone => ControlFlow::Done,
            TuiEvent::Quit => ControlFlow::Quit,
        }
    }

    pub fn files(&self) -> Vec<&FileState> {
        let mut sorted: Vec<&FileState> = self.files.iter().collect();
        sorted.sort_by(|a, b| a.ratio().total_cmp(&b.ratio()));
        sorted
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
        }));
        assert!(app.files()[0].done);
    }

    #[test]
    fn test_files_sorted_by_ratio() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/high.bin".to_string(),
            total_pages: 100,
            residency: vec![true; 90],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/low.bin".to_string(),
            total_pages: 100,
            residency: vec![true; 10],
        }));
        let files = app.files();
        assert!(files[0].ratio() <= files[1].ratio());
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
