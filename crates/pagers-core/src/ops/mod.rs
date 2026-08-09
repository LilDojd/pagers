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

use crate::Cancellation;
use crate::mincore::{DefaultPageMap, PageMap};

pub use evict::Evict;
pub use lock::{Lock, LockedFile};
pub use lockall::Lockall;
pub use process::{CountsResult, FullResult, file_info};
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
    cancellation: Cancellation,
    on_progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    residency: Option<&'a PM>,
}

impl<'a, PM: PageMap> FileContext<'a, PM> {
    pub(crate) fn new(prepared: PreparedFile, cancellation: Cancellation) -> Self {
        Self {
            prepared,
            cancellation,
            on_progress: None,
            residency: None,
        }
    }

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
        self.prepared.offset()
    }

    pub fn len(&self) -> usize {
        self.prepared.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prepared.mmap.is_empty()
    }

    pub fn total_pages(&self) -> usize {
        self.prepared.total_pages()
    }

    pub fn residency(&self) -> Option<&PM> {
        self.residency
    }

    pub fn report_progress(&self, pages_walked: usize, action_count: usize) {
        if let Some(on_progress) = self.on_progress {
            on_progress(pages_walked, action_count);
        }
    }

    pub fn check_cancelled(&self) -> crate::Result<()> {
        self.cancellation.check()
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
    offset: u64,
    max_len: Option<u64>,
}

impl FileRange {
    pub fn full() -> Self {
        Self {
            offset: 0,
            max_len: None,
        }
    }

    pub fn new(offset: u64, max_len: Option<u64>) -> crate::Result<Self> {
        let range = Self { offset, max_len };
        range.validate()?;
        Ok(range)
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn max_len(&self) -> Option<u64> {
        self.max_len
    }

    pub(crate) fn validate(&self) -> crate::Result<()> {
        let page_size = *crate::pagesize::PAGE_SIZE as u64;
        if !self.offset.is_multiple_of(page_size) {
            return Err(crate::Error::UnalignedRange {
                offset: self.offset,
                page_size,
            });
        }
        if self.max_len == Some(0) {
            return Err(crate::Error::EmptyRange);
        }
        if let Some(max_len) = self.max_len
            && self.offset.checked_add(max_len).is_none()
        {
            return Err(crate::Error::RangeOverflow {
                offset: self.offset,
                max_len,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileInfo<PM = DefaultPageMap> {
    pub total_pages: usize,
    pub residency: PM,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub total_pages: AtomicUsize,
    pub initial_pages_in_core: AtomicUsize,
    pub action_pages: AtomicUsize,
    pub total_files: AtomicUsize,
    pub total_dirs: AtomicUsize,
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use crate::Cancellation;

    use super::{FileContext, FileRange, Op, ResidencyEffect, Touch, prepare_file};

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

        let context: FileContext<'_, Vec<bool>> = FileContext::new(prepared, Cancellation::new());

        assert_eq!(context.len(), mmap.len());
        assert_eq!(context.path(), file.path());
        assert_eq!(context.total_pages(), 1);
    }

    #[test]
    fn touch_stops_when_cancelled_during_page_walk() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&vec![0; *crate::pagesize::PAGE_SIZE * 512])
            .unwrap();
        let prepared = prepare_file(file.path(), &FileRange::full())
            .unwrap()
            .unwrap();
        let cancellation = Cancellation::new();
        let context_cancellation = cancellation.clone();
        let cancel_after_progress = |_: usize, _: usize| {
            cancellation.cancel();
        };
        let context: FileContext<'_, Vec<bool>> = FileContext::new(prepared, context_cancellation)
            .with_progress(Some(&cancel_after_progress));

        let result = Touch.execute(&context);

        assert!(matches!(result, Err(crate::Error::Cancelled)));
    }
}
