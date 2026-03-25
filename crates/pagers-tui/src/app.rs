use std::collections::HashMap;
use std::sync::Arc;

use pagers_core::events::Event as CoreEvent;
use pagers_core::mincore::{DefaultPageMap, PageMap, PageMapSlice as _};

use crate::event::TuiEvent;
use crate::state::FileState;

pub struct App<PM: PageMap = DefaultPageMap> {
    files: Vec<FileState<PM>>,
    file_index: HashMap<Arc<str>, usize>,
}

pub enum ControlFlow {
    Continue,
    Quit,
    Done,
}

impl<PM: PageMap> Default for App<PM> {
    fn default() -> Self {
        Self::new()
    }
}

impl<PM: PageMap> App<PM> {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            file_index: HashMap::new(),
        }
    }

    pub(crate) fn handle_event(&mut self, event: TuiEvent<PM>) -> ControlFlow {
        match event {
            TuiEvent::Core(CoreEvent::FileStart {
                path,
                total_pages,
                residency,
            }) => {
                let pages_in_core = residency.count_filled();
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
            TuiEvent::Core(CoreEvent::FileProgress {
                path,
                page_offset,
                pages_walked,
                resident,
            }) => {
                if let Some(&idx) = self.file_index.get(&path) {
                    let file = &mut self.files[idx];
                    let start = page_offset;
                    let end = (page_offset + pages_walked).min(file.residency.len());
                    if start < end {
                        let was_set = file.residency[start..end].count_filled();
                        file.residency[start..end].fill(resident);
                        let now_set = if resident { end - start } else { 0 };
                        file.pages_in_core = file.pages_in_core - was_set + now_set;
                    }
                }
                ControlFlow::Continue
            }
            TuiEvent::Core(CoreEvent::FileDone { path }) => {
                if let Some(&idx) = self.file_index.get(&path) {
                    self.files[idx].done = true;
                }
                ControlFlow::Continue
            }
            TuiEvent::Core(CoreEvent::AllDone) => ControlFlow::Done,
            TuiEvent::Quit => ControlFlow::Quit,
        }
    }

    pub fn files(&self) -> Vec<&FileState<PM>> {
        self.files.iter().collect()
    }

    /// Return files for the live TUI viewport: sorted by size descending
    /// (path tiebreaker), with done files hidden when total exceeds `max`.
    pub fn visible_files(&self, max: usize) -> Vec<&FileState<PM>> {
        let mut files: Vec<&FileState<PM>> = self.files.iter().collect();
        files.sort_by(|a, b| {
            b.total_pages
                .cmp(&a.total_pages)
                .then_with(|| a.path.cmp(&b.path))
        });
        if files.len() > max {
            files.retain(|f| !f.done);
            if files.is_empty() {
                files = self.files.iter().collect();
                files.sort_by(|a, b| {
                    b.total_pages
                        .cmp(&a.total_pages)
                        .then_with(|| a.path.cmp(&b.path))
                });
            }
        }
        files.truncate(max);
        files
    }

    pub fn into_files(self) -> Vec<FileState<PM>> {
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
            path: "/test.bin".into(),
            total_pages: 100,
            residency: bitvec::bitvec![1; 50],
        }));
        assert!(matches!(flow, ControlFlow::Continue));
        assert_eq!(app.files().len(), 1);
        assert_eq!(app.files()[0].pages_in_core, 50);
    }

    #[test]
    fn test_handle_file_progress_uses_hashmap() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/a.bin".into(),
            total_pages: 100,
            residency: bitvec::bitvec![0; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileProgress {
            path: "/a.bin".into(),
            page_offset: 0,
            pages_walked: 100,
            resident: true,
        }));
        assert_eq!(app.files()[0].pages_in_core, 100);
    }

    #[test]
    fn test_handle_file_done_sets_flag() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/a.bin".into(),
            total_pages: 100,
            residency: bitvec::bitvec![0; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone {
            path: "/a.bin".into(),
        }));
        assert!(app.files()[0].done);
    }

    #[test]
    fn test_files_in_insertion_order() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/first.bin".into(),
            total_pages: 100,
            residency: bitvec::bitvec![1; 90],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/second.bin".into(),
            total_pages: 100,
            residency: bitvec::bitvec![1; 10],
        }));
        let files = app.files();
        assert_eq!(&*files[0].path, "/first.bin");
        assert_eq!(&*files[1].path, "/second.bin");
    }

    #[test]
    fn test_visible_files_sorted_by_size_desc() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/small.bin".into(),
            total_pages: 10,
            residency: bitvec::bitvec![0; 10],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/big.bin".into(),
            total_pages: 1000,
            residency: bitvec::bitvec![0; 1000],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/mid.bin".into(),
            total_pages: 100,
            residency: bitvec::bitvec![0; 100],
        }));
        let vis = app.visible_files(8);
        assert_eq!(&*vis[0].path, "/big.bin");
        assert_eq!(&*vis[1].path, "/mid.bin");
        assert_eq!(&*vis[2].path, "/small.bin");
    }

    #[test]
    fn test_visible_files_hides_done_when_overflow() {
        let mut app = App::new();
        // Add 3 files, mark one done, max=2
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/a.bin".into(),
            total_pages: 100,
            residency: bitvec::bitvec![0; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/b.bin".into(),
            total_pages: 200,
            residency: bitvec::bitvec![0; 200],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/c.bin".into(),
            total_pages: 50,
            residency: bitvec::bitvec![0; 50],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone {
            path: "/b.bin".into(),
        }));
        let vis = app.visible_files(2);
        assert_eq!(vis.len(), 2);
        // /b.bin is done and should be hidden since 3 > 2
        assert!(vis.iter().all(|f| &*f.path != "/b.bin"));
    }

    #[test]
    fn test_visible_files_keeps_done_when_fits() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            path: "/a.bin".into(),
            total_pages: 100,
            residency: bitvec::bitvec![0; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone {
            path: "/a.bin".into(),
        }));
        // Only 1 file, max=8 → done file stays visible
        let vis = app.visible_files(8);
        assert_eq!(vis.len(), 1);
        assert!(vis[0].done);
    }

    #[test]
    fn test_visible_files_all_done_overflow_shows_largest() {
        let mut app = App::new();
        for i in 0..3 {
            let path: Arc<str> = format!("/{i}.bin").into();
            app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
                path: path.clone(),
                total_pages: (i + 1) * 100,
                residency: bitvec::bitvec![0; (i + 1) * 100],
            }));
            app.handle_event(TuiEvent::Core(CoreEvent::FileDone { path }));
        }
        let vis = app.visible_files(2);
        assert_eq!(vis.len(), 2);
        assert_eq!(&*vis[0].path, "/2.bin");
        assert_eq!(&*vis[1].path, "/1.bin");
    }

    #[test]
    fn test_all_done_returns_done() {
        let mut app: App<Vec<bool>> = App::new();
        let flow = app.handle_event(TuiEvent::Core(CoreEvent::AllDone));
        assert!(matches!(flow, ControlFlow::Done));
    }

    #[test]
    fn test_quit_returns_quit() {
        let mut app: App<Vec<bool>> = App::new();
        let flow = app.handle_event(TuiEvent::Quit);
        assert!(matches!(flow, ControlFlow::Quit));
    }
}
