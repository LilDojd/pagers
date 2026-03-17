pub struct FileState {
    pub path: String,
    pub total_pages: usize,
    pub pages_in_core: usize,
    pub residency: Vec<bool>,
    pub done: bool,
}

impl FileState {
    pub fn ratio(&self) -> f64 {
        if self.total_pages == 0 {
            return 0.0;
        }
        self.pages_in_core as f64 / self.total_pages as f64
    }

    /// Downsample the residency bitmap into `width` buckets.
    /// Returns a vec of (cached_count, total_count) per bucket.
    pub fn bucketize(&self, width: usize) -> Vec<(usize, usize)> {
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

                (slice.iter().filter(|&&b| b).count(), slice.len())
            })
            .collect()
    }
}
