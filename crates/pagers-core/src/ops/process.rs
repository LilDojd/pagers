use std::fs::File;
use std::path::{Path, PathBuf};
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
    pub pages_in_core_before: usize,
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
    fn pages_in_core_before(&self) -> Option<usize> {
        Some(self.pages_in_core_before)
    }
    fn pages_in_core_after(&self) -> usize {
        self.pages_in_core_after
    }
}

pub(crate) struct PreparedFile {
    pub path: PathBuf,
    pub file: File,
    pub mmap: Arc<memmap2::Mmap>,
    range: ResolvedRange,
}

impl PreparedFile {
    pub(crate) fn offset(&self) -> u64 {
        self.range.offset
    }

    pub(crate) fn len(&self) -> usize {
        self.range.len
    }

    pub(crate) fn total_pages(&self) -> usize {
        self.range.len.div_ceil(*crate::pagesize::PAGE_SIZE)
    }
}

struct ResolvedRange {
    offset: u64,
    len: usize,
}

impl ResolvedRange {
    fn for_file(path: &Path, file_len: u64, range: &FileRange) -> crate::Result<Option<Self>> {
        range.validate()?;
        if file_len == 0 {
            return Ok(None);
        }

        let offset = range.offset();
        if offset >= file_len {
            return Err(Error::OffsetBeyondFile {
                path: path.to_path_buf(),
                offset,
                file_len,
            });
        }

        let available = file_len - offset;
        let len = range.max_len().unwrap_or(available).min(available);
        Ok(Some(Self {
            offset,
            len: len.try_into()?,
        }))
    }
}

pub(crate) fn prepare_file(path: &Path, range: &FileRange) -> crate::Result<Option<PreparedFile>> {
    let io_err = |e| Error::io(path.display().to_string(), e);

    let file = fs_err::File::open(path).map_err(io_err)?;
    let file_len = file.metadata().map_err(io_err)?.len();

    let Some(resolved) = ResolvedRange::for_file(path, file_len, range)? else {
        return Ok(None);
    };

    let mmap = Arc::new(unsafe {
        MmapOptions::new()
            .offset(resolved.offset)
            .len(resolved.len)
            .map(file.file())
            .map_err(io_err)?
    });

    Ok(Some(PreparedFile {
        path: path.to_path_buf(),
        file: file.into_file(),
        mmap,
        range: resolved,
    }))
}

pub fn file_info<PM: PageMap>(
    path: &Path,
    range: &FileRange,
) -> crate::Result<Option<FileInfo<PM>>> {
    let Some(prepared) = prepare_file(path, range)? else {
        return Ok(None);
    };
    let residency: PM = crate::mincore::residency(&prepared.mmap, prepared.len())?;
    Ok(Some(FileInfo {
        total_pages: prepared.total_pages(),
        residency,
    }))
}

#[cfg(test)]
mod tests {
    use crate::mincore::PageMapSlice as _;
    use crate::mode;

    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::super::*;

    struct AdvisoryNoop;

    impl Op for AdvisoryNoop {
        const LABEL: &str = "advisory noop";
        const EFFECT: ResidencyEffect = ResidencyEffect::EvictAdvisory;
        type Output = ();

        fn execute<PM: PageMap + Sync>(&self, _ctx: &FileContext<'_, PM>) -> crate::Result<()> {
            Ok(())
        }
    }

    fn create_temp_file(pages: usize) -> (tempfile::NamedTempFile, usize) {
        let page_size = *crate::pagesize::PAGE_SIZE;
        let size = page_size * pages;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&vec![0xABu8; size]).unwrap();
        f.flush().unwrap();
        (f, size)
    }

    fn not_cancelled() -> crate::Cancellation<'static> {
        static FLAG: AtomicBool = AtomicBool::new(false);
        crate::Cancellation::new(&FLAG)
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
                    let result = mode::counts_process_file::<Query, $t>(
                        &Query,
                        f.path(),
                        &range,
                        not_cancelled(),
                    )
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
                    let result: Option<CountsResult<()>> = mode::counts_process_file::<Query, $t>(
                        &Query,
                        f.path(),
                        &range,
                        not_cancelled(),
                    )
                    .unwrap();
                    assert!(result.is_none());
                }

                #[test]
                fn offset_beyond_file() {
                    let (f, _) = create_temp_file(1);
                    let range = FileRange {
                        offset: (*crate::pagesize::PAGE_SIZE * 100) as u64,
                        max_len: None,
                    };
                    let result: crate::Result<Option<CountsResult<()>>> =
                        mode::counts_process_file::<Query, $t>(
                            &Query,
                            f.path(),
                            &range,
                            not_cancelled(),
                        );
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
                    let result = mode::counts_process_file::<Query, $t>(
                        &Query,
                        f.path(),
                        &range,
                        not_cancelled(),
                    )
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

                    mode::counts_process_file::<Evict, $t>(
                        &Evict,
                        f.path(),
                        &range,
                        not_cancelled(),
                    )
                    .unwrap();
                    mode::counts_process_file::<Touch, $t>(
                        &Touch,
                        f.path(),
                        &range,
                        not_cancelled(),
                    )
                    .unwrap();

                    let file = fs_err::File::open(f.path()).unwrap();
                    let mmap_check = unsafe {
                        memmap2::MmapOptions::new()
                            .len(size)
                            .map(file.file())
                            .unwrap()
                    };
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
                        mode::counts_process_file::<Evict, $t>(
                            &Evict,
                            f.path(),
                            &range,
                            not_cancelled(),
                        );
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
                        mode::full_process_file::<Query, $t>(
                            &Query,
                            f.path(),
                            &range,
                            None,
                            None,
                            not_cancelled(),
                        )
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
                        mode::full_process_file::<Query, $t>(
                            &Query,
                            f.path(),
                            &range,
                            None,
                            None,
                            not_cancelled(),
                        )
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
                    let result = mode::counts_process_file::<Query, $t>(
                        &Query,
                        f.path(),
                        &range,
                        not_cancelled(),
                    )
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
                            not_cancelled(),
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

    #[test]
    fn zero_length_range_is_rejected() {
        let (file, _) = create_temp_file(1);
        let range = FileRange {
            offset: 0,
            max_len: Some(0),
        };

        assert!(matches!(
            prepare_file(file.path(), &range),
            Err(Error::EmptyRange)
        ));
    }

    #[test]
    fn unaligned_range_is_rejected() {
        let (file, _) = create_temp_file(2);
        let range = FileRange {
            offset: 1,
            max_len: Some(*crate::pagesize::PAGE_SIZE as u64),
        };

        assert!(matches!(
            prepare_file(file.path(), &range),
            Err(Error::UnalignedRange { .. })
        ));
    }

    #[test]
    fn overflowing_range_is_rejected_without_panicking() {
        let (file, _) = create_temp_file(2);
        let range = FileRange {
            offset: *crate::pagesize::PAGE_SIZE as u64,
            max_len: Some(u64::MAX),
        };

        assert!(matches!(
            prepare_file(file.path(), &range),
            Err(Error::RangeOverflow { .. })
        ));
    }

    #[test]
    fn advisory_effect_measures_the_after_state() {
        let (f, _) = create_temp_file(4);
        let range = FileRange {
            offset: 0,
            max_len: None,
        };

        let result = mode::counts_process_file::<AdvisoryNoop, Vec<bool>>(
            &AdvisoryNoop,
            f.path(),
            &range,
            not_cancelled(),
        )
        .unwrap()
        .unwrap();

        assert!(result.pages_in_core_before > 0);
        assert_eq!(
            result.pages_in_core_after, result.pages_in_core_before,
            "an advisory operation must measure rather than assume its after-state"
        );
    }
}
