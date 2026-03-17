/// Tracks the state of a single file being processed.
pub struct FileState {
    pub path: String,
    pub total_pages: usize,
    pub pages_in_core: usize,
    pub done: bool,
}

impl FileState {
    /// Returns the ratio of pages in core to total pages (0.0 to 1.0).
    pub fn ratio(&self) -> f64 {
        if self.total_pages == 0 {
            return 0.0;
        }
        self.pages_in_core as f64 / self.total_pages as f64
    }
}
