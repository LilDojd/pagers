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
pub(crate) use process::{CountsResult, FileProcessed, FullResult, SkipResult, file_info};
pub(crate) use process::{PreparedFile, prepare_file};
pub use query::Query;
pub use touch::Touch;

pub trait Op: Sync {
    const LABEL: &str;
    const MUTATES_RESIDENCY: bool = true;
    /// Skip all residency checks; the after-state is known to be empty.
    const SKIP_RESIDENCY: bool = false;
    /// +1 for ops that add pages to cache (touch/lock), -1 for evict, 0 for query.
    const ACTION_SIGN: isize = 0;

    type Output: Send;
    fn execute<PM: PageMap + Sync>(&self, ctx: &FileContext<'_, PM>)
    -> crate::Result<Self::Output>;

    fn finish(&self) -> crate::Result<()> {
        Ok(())
    }

    fn action_pages(
        _output: &Self::Output,
        _total_pages: usize,
        _pages_in_core_before: Option<usize>,
        _pages_in_core_after: usize,
    ) -> usize {
        0
    }
}

pub struct FileContext<'a, PM: PageMap = DefaultPageMap> {
    pub(crate) file: &'a File,
    pub(crate) path: &'a Path,
    pub(crate) mmap: Arc<Mmap>,
    pub(crate) offset: u64,
    pub(crate) len: usize,
    pub(crate) on_progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    pub(crate) residency: Option<&'a PM>,
}

impl<PM: PageMap> std::fmt::Debug for FileContext<'_, PM> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileContext")
            .field("path", &self.path)
            .field("offset", &self.offset)
            .field("len", &self.len)
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
    pub(crate) fn is_full_file(&self) -> bool {
        self.offset == 0 && self.max_len.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileInfo<PM = DefaultPageMap> {
    total_pages: usize,
    residency: PM,
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
