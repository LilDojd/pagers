use std::sync::Arc;

use pagers_core::mincore::{DefaultPageMap, PageMap, PageMapSlice as _};

pub(crate) struct FileState<PM: PageMap = DefaultPageMap> {
    pub path: Arc<str>,
    pub total_pages: usize,
    pub pages_in_core: usize,
    pub residency: PM,
    pub done: bool,
}

impl<PM: PageMap> FileState<PM> {
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
