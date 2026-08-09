use std::sync::Arc;

use pagers_core::mincore::{DefaultPageMap, PageMap, PageMapSlice as _};

pub struct FileState<PM: PageMap = DefaultPageMap> {
    pub(crate) path: Arc<str>,
    pub(crate) total_pages: usize,
    pub(crate) pages_in_core: usize,
    pub(crate) residency: PM,
    pub(crate) done: bool,
}

impl<PM: PageMap> FileState<PM> {
    pub(crate) fn ratio(&self) -> f64 {
        if self.total_pages == 0 {
            return 0.0;
        }
        self.pages_in_core as f64 / self.total_pages as f64
    }

    /// Downsample the residency bitmap into `width` buckets.
    /// Returns a vec of (cached_count, total_count) per bucket.
    pub(crate) fn bucketize(&self, width: usize) -> Vec<(usize, usize)> {
        let n = self.total_pages;

        if width == 0 || n == 0 {
            return Vec::new();
        }
        let w = width.min(n);
        (0..w)
            .map(|i| {
                let start = i * n / w;
                let end = (i + 1) * n / w;
                let slice = &self.residency[start..end];

                (slice.count_filled(), slice.len())
            })
            .collect()
    }
}
