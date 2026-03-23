mod evict;
mod lock;
mod lockall;
mod process;
mod query;
mod touch;

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;

use memmap2::Mmap;

use crate::events::EventSink;

pub use evict::Evict;
pub use lock::{Lock, LockedFile};
pub use lockall::Lockall;
pub use process::{process_file, send_file_start};
pub use query::Query;
pub use touch::Touch;

pub trait Op: Sync {
    const LABEL: &str;

    type Output: Send;
    fn execute(&self, ctx: &FileContext) -> crate::Result<Self::Output>;

    fn finish(&self) -> crate::Result<()> {
        Ok(())
    }
}

pub struct FileContext<'a> {
    pub file: &'a File,
    pub path: &'a Path,
    pub mmap: Arc<Mmap>,
    pub offset: u64,
    pub len: usize,
    pub events: Option<&'a EventSink>,
}

#[derive(Clone, Copy)]
pub struct FileRange {
    pub offset: u64,
    pub max_len: Option<u64>,
}

pub struct Stats {
    pub total_pages: AtomicI64,
    pub total_pages_in_core: AtomicI64,
    pub total_files: AtomicI64,
    pub total_dirs: AtomicI64,
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

impl Stats {
    pub fn new() -> Self {
        Self {
            total_pages: AtomicI64::new(0),
            total_pages_in_core: AtomicI64::new(0),
            total_files: AtomicI64::new(0),
            total_dirs: AtomicI64::new(0),
        }
    }
}
