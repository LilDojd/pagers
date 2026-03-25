use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use memmap2::MmapOptions;

use super::{FileInfo, FileRange};
use crate::Error;
use crate::mincore::PageMap;

pub trait FileProcessed {
    type Output;
    fn into_output(self) -> Self::Output;
    fn output_ref(&self) -> &Self::Output;
    fn total_pages(&self) -> usize;
    fn pages_in_core_before(&self) -> Option<usize> {
        None
    }
    fn pages_in_core_after(&self) -> usize;
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullResult<O, PM> {
    pub output: O,
    pub total_pages: usize,
    pub pages_in_core_before: usize,
    pub pages_in_core_after: usize,
    pub residency_before: Option<PM>,
    pub residency_after: Option<PM>,
}

impl<O, PM> FileProcessed for FullResult<O, PM> {
    type Output = O;
    fn into_output(self) -> O {
        self.output
    }
    fn output_ref(&self) -> &O {
        &self.output
    }
    fn total_pages(&self) -> usize {
        self.total_pages
    }
    fn pages_in_core_before(&self) -> Option<usize> {
        Some(self.pages_in_core_before)
    }
    fn pages_in_core_after(&self) -> usize {
        self.pages_in_core_after
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CountsResult<O> {
    pub output: O,
    pub total_pages: usize,
    pub pages_in_core_after: usize,
}

impl<O> FileProcessed for CountsResult<O> {
    type Output = O;
    fn into_output(self) -> O {
        self.output
    }
    fn output_ref(&self) -> &O {
        &self.output
    }
    fn total_pages(&self) -> usize {
        self.total_pages
    }
    fn pages_in_core_after(&self) -> usize {
        self.pages_in_core_after
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkipResult<O> {
    pub output: O,
    pub total_pages: usize,
}

impl<O> FileProcessed for SkipResult<O> {
    type Output = O;
    fn into_output(self) -> O {
        self.output
    }
    fn output_ref(&self) -> &O {
        &self.output
    }
    fn total_pages(&self) -> usize {
        self.total_pages
    }
    fn pages_in_core_after(&self) -> usize {
        0
    }
}

pub(crate) struct PreparedFile {
    pub file: File,
    pub offset: u64,
    pub len: usize,
    pub total_pages: usize,
    pub mmap: Arc<memmap2::Mmap>,
}

pub(crate) fn prepare_file(path: &Path, range: &FileRange) -> crate::Result<Option<PreparedFile>> {
    let io_err = |e| Error::io(path.display().to_string(), e);

    let file = File::open(path).map_err(io_err)?;
    let file_len = file.metadata().map_err(io_err)?.len();

    if file_len == 0 {
        return Ok(None);
    }

    let (offset, len) =
        effective_range(file_len, range).ok_or_else(|| Error::OffsetBeyondFile {
            path: path.to_path_buf(),
            offset: range.offset,
            file_len,
        })?;

    let total_pages = len.div_ceil(*crate::pagesize::PAGE_SIZE);

    let mmap = Arc::new(unsafe {
        MmapOptions::new()
            .offset(offset)
            .len(len)
            .map(&file)
            .map_err(io_err)?
    });

    Ok(Some(PreparedFile {
        file,
        offset,
        len,
        total_pages,
        mmap,
    }))
}

fn effective_range(file_len: u64, range: &FileRange) -> Option<(u64, usize)> {
    if file_len == 0 {
        return None;
    }
    let offset = range.offset;
    if offset >= file_len {
        return None;
    }
    let len = match range.max_len {
        Some(max) if (offset + max) < file_len => max as usize,
        _ => (file_len - offset) as usize,
    };
    Some((offset, len))
}

pub fn file_info<PM: PageMap>(
    path: &Path,
    range: &FileRange,
) -> crate::Result<Option<FileInfo<PM>>> {
    let io_err = |e| Error::io(path.display().to_string(), e);

    let file = File::open(path).map_err(io_err)?;
    let file_len = file.metadata().map_err(io_err)?.len();

    let Some((offset, len)) = effective_range(file_len, range) else {
        return Ok(None);
    };

    let total_pages = len.div_ceil(*crate::pagesize::PAGE_SIZE);
    let mmap = unsafe {
        MmapOptions::new()
            .offset(offset)
            .len(len)
            .map(&file)
            .map_err(io_err)?
    };
    let residency: PM = crate::mincore::residency(&mmap, len)?;
    Ok(Some(FileInfo {
        total_pages,
        residency,
    }))
}

#[cfg(test)]
mod tests {
    use crate::mincore::PageMapSlice as _;
    use crate::mode;

    use super::*;
    use std::io::Write;
    use std::sync::atomic::Ordering;

    use super::super::*;

    fn create_temp_file(pages: usize) -> (tempfile::NamedTempFile, usize) {
        let page_size = *crate::pagesize::PAGE_SIZE;
        let size = page_size * pages;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&vec![0xABu8; size]).unwrap();
        f.flush().unwrap();
        (f, size)
    }

    macro_rules! process_tests {
        ($t:ty, $mod:ident) => {
            mod $mod {
                use super::*;

                #[test]
                fn query_counts_pages() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result = mode::counts_process_file::<Query, $t>(&Query, f.path(), &range)
                        .unwrap()
                        .unwrap();
                    assert_eq!(result.total_pages, 4);
                }

                #[test]
                fn empty_file_returns_none() {
                    let f = tempfile::NamedTempFile::new().unwrap();
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: Option<CountsResult<()>> =
                        mode::counts_process_file::<Query, $t>(&Query, f.path(), &range).unwrap();
                    assert!(result.is_none());
                }

                #[test]
                fn offset_beyond_file() {
                    let (f, _) = create_temp_file(1);
                    let range = FileRange {
                        offset: 1_000_000,
                        max_len: None,
                    };
                    let result: crate::Result<Option<CountsResult<()>>> =
                        mode::counts_process_file::<Query, $t>(&Query, f.path(), &range);
                    assert!(result.is_err());
                    let err = result.unwrap_err();
                    assert!(
                        matches!(err, Error::OffsetBeyondFile { .. }),
                        "expected OffsetBeyondFile, got: {err}"
                    );
                }

                #[test]
                fn with_max_len() {
                    let (f, _) = create_temp_file(8);
                    let page_size = *crate::pagesize::PAGE_SIZE;
                    let range = FileRange {
                        offset: 0,
                        max_len: Some((page_size * 2) as u64),
                    };
                    let result = mode::counts_process_file::<Query, $t>(&Query, f.path(), &range)
                        .unwrap()
                        .unwrap();
                    assert_eq!(result.total_pages, 2);
                }

                #[test]
                fn touch_makes_resident() {
                    let (f, size) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };

                    mode::counts_process_file::<Evict, $t>(&Evict, f.path(), &range).unwrap();
                    mode::counts_process_file::<Touch, $t>(&Touch, f.path(), &range).unwrap();

                    let file = std::fs::File::open(f.path()).unwrap();
                    let mmap_check =
                        unsafe { memmap2::MmapOptions::new().len(size).map(&file).unwrap() };
                    let residency: $t = crate::mincore::residency(&mmap_check, size).unwrap();
                    assert!(
                        (0..residency.len()).all(|i| residency[i..i + 1].count_filled() == 1),
                        "expected all pages resident after touch"
                    );
                }

                #[test]
                fn evict_succeeds() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: crate::Result<Option<CountsResult<()>>> =
                        mode::counts_process_file::<Evict, $t>(&Evict, f.path(), &range);
                    assert!(result.is_ok());
                }

                #[test]
                fn full_residency() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: FullResult<(), $t> =
                        mode::full_process_file::<Query, $t>(&Query, f.path(), &range, None, None)
                            .unwrap()
                            .unwrap();
                    assert!(result.residency_after.is_some());
                    assert_eq!(result.residency_after.unwrap().len(), 4);
                }

                #[test]
                fn query_full_reuses_before() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: FullResult<(), $t> =
                        mode::full_process_file::<Query, $t>(&Query, f.path(), &range, None, None)
                            .unwrap()
                            .unwrap();
                    assert_eq!(result.pages_in_core_before, result.pages_in_core_after);
                    assert!(result.residency_before.is_none());
                    assert!(result.residency_after.is_some());
                }

                #[test]
                fn counts_without_bitmap() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result = mode::counts_process_file::<Query, $t>(&Query, f.path(), &range)
                        .unwrap()
                        .unwrap();
                    assert_eq!(result.total_pages, 4);
                }

                #[test]
                fn nonexistent_returns_error() {
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: crate::Result<Option<CountsResult<()>>> =
                        mode::counts_process_file::<Query, $t>(
                            &Query,
                            std::path::Path::new("/nonexistent/file.dat"),
                            &range,
                        );
                    assert!(result.is_err());
                }

                #[test]
                fn file_info_pages() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let info: FileInfo<$t> = file_info(f.path(), &range).unwrap().unwrap();
                    assert_eq!(info.total_pages, 4);
                    assert_eq!(info.residency.len(), 4);
                }
            }
        };
    }

    process_tests!(Vec<bool>, vec_bool_impl);

    #[cfg(feature = "bitvec")]
    process_tests!(::bitvec::vec::BitVec, bitvec_impl);

    #[test]
    fn test_stats_default() {
        let stats = Stats::default();
        assert_eq!(stats.total_pages.load(Ordering::Relaxed), 0);
        assert_eq!(stats.initial_pages_in_core.load(Ordering::Relaxed), 0);
        assert_eq!(stats.action_pages.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_files.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_dirs.load(Ordering::Relaxed), 0);
    }
}
