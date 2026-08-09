use std::collections::HashMap;

use pagers_core::events::Event as CoreEvent;
use pagers_core::mincore::{DefaultPageMap, PageMap, PageMapSlice as _};

use crate::event::TuiEvent;
use crate::state::FileState;

pub(crate) struct App<PM: PageMap = DefaultPageMap> {
    files: Vec<FileState<PM>>,
    file_index: HashMap<usize, usize>,
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
    pub(crate) fn new() -> Self {
        Self {
            files: Vec::new(),
            file_index: HashMap::new(),
        }
    }

    pub(crate) fn handle_event(&mut self, event: TuiEvent<PM>) -> ControlFlow {
        match event {
            TuiEvent::Core(CoreEvent::FileStart {
                id,
                path,
                total_pages,
                residency,
            }) => {
                let pages_in_core = residency.count_filled();
                let idx = self.files.len();
                self.file_index.insert(id, idx);
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
                id,
                page_offset,
                pages_walked,
                resident,
            }) => {
                if let Some(&idx) = self.file_index.get(&id) {
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
            TuiEvent::Core(CoreEvent::FileDone { id }) => {
                if let Some(&idx) = self.file_index.get(&id) {
                    self.files[idx].done = true;
                    self.trim_completed();
                }
                ControlFlow::Continue
            }
            TuiEvent::Core(CoreEvent::AllDone) => ControlFlow::Done,
            TuiEvent::Quit => ControlFlow::Quit,
        }
    }

    #[cfg(test)]
    fn files(&self) -> Vec<&FileState<PM>> {
        self.files.iter().collect()
    }

    /// Return files for the live TUI viewport: sorted by size descending
    /// (path tiebreaker), with done files hidden when total exceeds `max`.
    pub(crate) fn visible_files(&self, max: usize) -> Vec<&FileState<PM>> {
        let mut files: Vec<&FileState<PM>> = self.files.iter().collect();
        files.sort_by(|a, b| {
            b.total_pages
                .cmp(&a.total_pages)
                .then_with(|| a.path.cmp(&b.path))
        });
        if files.len() > max && files.iter().any(|file| !file.done) {
            files.retain(|f| !f.done);
        }
        files.truncate(max);
        files
    }

    fn trim_completed(&mut self) {
        if self.files.iter().filter(|file| file.done).count() <= crate::MAX_DISPLAY_FILES as usize {
            return;
        }
        let remove = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| file.done)
            .min_by(|(_, a), (_, b)| {
                a.total_pages
                    .cmp(&b.total_pages)
                    .then_with(|| b.path.cmp(&a.path))
            })
            .map(|(index, _)| index)
            .expect("at least one completed file");
        self.files.remove(remove);
        self.file_index.retain(|_, index| {
            if *index == remove {
                return false;
            }
            if *index > remove {
                *index -= 1;
            }
            true
        });
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
            id: 0,
            path: "/test.bin".into(),
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
            id: 0,
            path: "/a.bin".into(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileProgress {
            id: 0,
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
            id: 0,
            path: "/a.bin".into(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone { id: 0 }));
        assert!(app.files()[0].done);
    }

    #[test]
    fn test_completed_files_are_bounded() {
        let mut app = App::new();
        for total_pages in 0..10 {
            app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
                id: total_pages,
                path: format!("/{total_pages}.bin").into(),
                total_pages,
                residency: vec![false; total_pages],
            }));
            app.handle_event(TuiEvent::Core(CoreEvent::FileDone { id: total_pages }));
        }

        let files = app.visible_files(usize::MAX);
        assert_eq!(files.len(), crate::MAX_DISPLAY_FILES as usize);
        assert_eq!(files.last().unwrap().total_pages, 2);
    }

    #[test]
    fn test_files_in_insertion_order() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            id: 0,
            path: "/first.bin".into(),
            total_pages: 100,
            residency: vec![true; 90],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            id: 1,
            path: "/second.bin".into(),
            total_pages: 100,
            residency: vec![true; 10],
        }));
        let files = app.files();
        assert_eq!(&*files[0].path, "/first.bin");
        assert_eq!(&*files[1].path, "/second.bin");
    }

    #[test]
    fn test_visible_files_sorted_by_size_desc() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            id: 0,
            path: "/small.bin".into(),
            total_pages: 10,
            residency: vec![false; 10],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            id: 1,
            path: "/big.bin".into(),
            total_pages: 1000,
            residency: vec![false; 1000],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            id: 2,
            path: "/mid.bin".into(),
            total_pages: 100,
            residency: vec![false; 100],
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
            id: 0,
            path: "/a.bin".into(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            id: 1,
            path: "/b.bin".into(),
            total_pages: 200,
            residency: vec![false; 200],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            id: 2,
            path: "/c.bin".into(),
            total_pages: 50,
            residency: vec![false; 50],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone { id: 1 }));
        let vis = app.visible_files(2);
        assert_eq!(vis.len(), 2);
        // /b.bin is done and should be hidden since 3 > 2
        assert!(vis.iter().all(|f| &*f.path != "/b.bin"));
    }

    #[test]
    fn test_visible_files_keeps_done_when_fits() {
        let mut app = App::new();
        app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
            id: 0,
            path: "/a.bin".into(),
            total_pages: 100,
            residency: vec![false; 100],
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone { id: 0 }));
        // Only 1 file, max=8 → done file stays visible
        let vis = app.visible_files(8);
        assert_eq!(vis.len(), 1);
        assert!(vis[0].done);
    }

    #[test]
    fn test_visible_files_all_done_overflow_shows_largest() {
        let mut app = App::new();
        for i in 0..3 {
            app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
                id: i,
                path: format!("/{i}.bin").into(),
                total_pages: (i + 1) * 100,
                residency: vec![false; (i + 1) * 100],
            }));
            app.handle_event(TuiEvent::Core(CoreEvent::FileDone { id: i }));
        }
        let vis = app.visible_files(2);
        assert_eq!(vis.len(), 2);
        assert_eq!(&*vis[0].path, "/2.bin");
        assert_eq!(&*vis[1].path, "/1.bin");
    }

    #[test]
    fn duplicate_paths_keep_independent_state() {
        let mut app = App::new();
        for id in 0..2 {
            app.handle_event(TuiEvent::Core(CoreEvent::FileStart {
                id,
                path: "/same.bin".into(),
                total_pages: 10,
                residency: vec![false; 10],
            }));
        }
        app.handle_event(TuiEvent::Core(CoreEvent::FileProgress {
            id: 0,
            page_offset: 0,
            pages_walked: 10,
            resident: true,
        }));
        app.handle_event(TuiEvent::Core(CoreEvent::FileDone { id: 0 }));

        let files = app.files();
        assert_eq!(files[0].pages_in_core, 10);
        assert!(files[0].done);
        assert_eq!(files[1].pages_in_core, 0);
        assert!(!files[1].done);
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
