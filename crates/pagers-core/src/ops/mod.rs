mod evict;
mod lock;
mod lockall;
mod process;
mod query;
mod touch;

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use memmap2::Mmap;

use crate::mincore::{DefaultPageMap, PageMap};

pub use evict::Evict;
pub use lock::{Lock, LockedFile};
pub use lockall::Lockall;
pub use process::{CountsResult, FileProcessed, FullResult, file_info};
pub(crate) use process::{PreparedFile, prepare_file};
pub use query::Query;
pub use touch::Touch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResidencyEffect {
    Preserve,
    Populate,
    EvictAdvisory,
}

impl ResidencyEffect {
    pub const fn action_sign(self) -> isize {
        match self {
            Self::Preserve => 0,
            Self::Populate => 1,
            Self::EvictAdvisory => -1,
        }
    }

    pub const fn has_action(self) -> bool {
        !matches!(self, Self::Preserve)
    }

    pub const fn progress_resident(self) -> Option<bool> {
        match self {
            Self::Preserve => None,
            Self::Populate => Some(true),
            Self::EvictAdvisory => Some(false),
        }
    }

    pub fn action_pages(self, before: usize, after: usize) -> usize {
        match self {
            Self::Preserve => 0,
            Self::Populate => after.saturating_sub(before),
            Self::EvictAdvisory => before.saturating_sub(after),
        }
    }
}

pub trait Op: Sync {
    const LABEL: &str;
    const EFFECT: ResidencyEffect;

    type Output: Send;
    fn execute<PM: PageMap + Sync>(&self, ctx: &FileContext<'_, PM>)
    -> crate::Result<Self::Output>;

    fn finish(&self) -> crate::Result<()> {
        Ok(())
    }
}

pub struct FileContext<'a, PM: PageMap = DefaultPageMap> {
    prepared: PreparedFile,
    on_progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    residency: Option<&'a PM>,
}

impl<'a, PM: PageMap> FileContext<'a, PM> {
    pub(crate) fn with_progress(
        mut self,
        on_progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    ) -> Self {
        self.on_progress = on_progress;
        self
    }

    pub(crate) fn with_residency(mut self, residency: Option<&'a PM>) -> Self {
        self.residency = residency;
        self
    }

    pub fn file(&self) -> &File {
        &self.prepared.file
    }

    pub fn path(&self) -> &Path {
        &self.prepared.path
    }

    pub fn mmap(&self) -> &Mmap {
        &self.prepared.mmap
    }

    pub fn mapping(&self) -> Arc<Mmap> {
        Arc::clone(&self.prepared.mmap)
    }

    pub fn offset(&self) -> u64 {
        self.prepared.offset
    }

    pub fn len(&self) -> usize {
        self.prepared.mmap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prepared.mmap.is_empty()
    }

    pub fn total_pages(&self) -> usize {
        self.prepared.total_pages
    }

    pub fn residency(&self) -> Option<&PM> {
        self.residency
    }

    pub fn report_progress(&self, pages_walked: usize, action_count: usize) {
        if let Some(on_progress) = self.on_progress {
            on_progress(pages_walked, action_count);
        }
    }
}

impl<'a, PM: PageMap> From<PreparedFile> for FileContext<'a, PM> {
    fn from(prepared: PreparedFile) -> Self {
        Self {
            prepared,
            on_progress: None,
            residency: None,
        }
    }
}

impl<PM: PageMap> std::fmt::Debug for FileContext<'_, PM> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileContext")
            .field("path", &self.path())
            .field("offset", &self.offset())
            .field("len", &self.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileRange {
    pub offset: u64,
    pub max_len: Option<u64>,
}

impl FileRange {
    pub fn is_full_file(&self) -> bool {
        self.offset == 0 && self.max_len.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileInfo<PM = DefaultPageMap> {
    pub total_pages: usize,
    pub residency: PM,
}

#[derive(Debug)]
pub struct Stats {
    pub total_pages: AtomicUsize,
    pub initial_pages_in_core: AtomicUsize,
    pub action_pages: AtomicUsize,
    pub total_files: AtomicUsize,
    pub total_dirs: AtomicUsize,
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

impl Stats {
    pub fn new() -> Self {
        Self {
            total_pages: AtomicUsize::new(0),
            initial_pages_in_core: AtomicUsize::new(0),
            action_pages: AtomicUsize::new(0),
            total_files: AtomicUsize::new(0),
            total_dirs: AtomicUsize::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use super::{FileContext, FileRange, ResidencyEffect, prepare_file};

    #[test]
    fn residency_effect_derives_sign_and_delta() {
        assert_eq!(ResidencyEffect::Preserve.action_sign(), 0);
        assert_eq!(ResidencyEffect::Preserve.action_pages(3, 7), 0);

        assert_eq!(ResidencyEffect::Populate.action_sign(), 1);
        assert_eq!(ResidencyEffect::Populate.action_pages(3, 7), 4);

        assert_eq!(ResidencyEffect::EvictAdvisory.action_sign(), -1);
        assert_eq!(ResidencyEffect::EvictAdvisory.action_pages(7, 3), 4);
        assert_eq!(ResidencyEffect::EvictAdvisory.action_pages(3, 7), 0);
    }

    #[test]
    fn file_context_from_prepared_file_preserves_mapping() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&[0; 64]).unwrap();
        let prepared = prepare_file(
            file.path(),
            &FileRange {
                offset: 0,
                max_len: None,
            },
        )
        .unwrap()
        .unwrap();
        let mmap = Arc::clone(&prepared.mmap);

        let context: FileContext<'_, Vec<bool>> = prepared.into();

        assert_eq!(context.len(), mmap.len());
        assert_eq!(context.path(), file.path());
        assert_eq!(context.total_pages(), 1);
    }
}
