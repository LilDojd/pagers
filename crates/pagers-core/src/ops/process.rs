use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use memmap2::MmapOptions;

use super::{FileContext, FileInfo, FileRange, FileResult, Op};
use crate::Error;
use crate::mincore::PageMap;

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

fn page_count<PM: PageMap>(
    file: &File,
    mmap: &memmap2::Mmap,
    offset: u64,
    len: usize,
) -> crate::Result<(i64, Option<PM>)> {
    if *crate::cachestat::SUPPORTED {
        use std::os::unix::io::AsFd;
        let count = crate::cachestat::cached_pages(file.as_fd(), offset, len as u64)? as i64;
        Ok((count, None))
    } else {
        let residency: PM = crate::mincore::residency(mmap, len)?;
        let count = residency.count_filled() as i64;
        Ok((count, Some(residency)))
    }
}

fn page_count_with_residency<PM: PageMap>(
    mmap: &memmap2::Mmap,
    len: usize,
) -> crate::Result<(i64, PM)> {
    let residency: PM = crate::mincore::residency(mmap, len)?;
    let count = residency.count_filled() as i64;
    Ok((count, residency))
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

pub fn process_file<O: Op, PM: PageMap>(
    op: &O,
    path: &Path,
    range: &FileRange,
    with_residency: bool,
) -> crate::Result<Option<FileResult<O::Output, PM>>> {
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

    let (pages_in_core_before, residency_before) = if with_residency {
        let (count, pm) = page_count_with_residency::<PM>(&mmap, len)?;
        (count, Some(pm))
    } else {
        let (count, _) = page_count::<PM>(&file, &mmap, offset, len)?;
        (count, None)
    };

    let ctx = FileContext {
        file: &file,
        path,
        mmap: Arc::clone(&mmap),
        offset,
        len,
    };

    let output = op.execute(&ctx)?;

    let (pages_in_core_after, residency_after) = if with_residency {
        let (count, pm) = page_count_with_residency::<PM>(&mmap, len)?;
        (count, Some(pm))
    } else {
        let (count, _) = page_count::<PM>(&file, &mmap, offset, len)?;
        (count, None)
    };

    Ok(Some(FileResult {
        output,
        total_pages,
        pages_in_core_before,
        pages_in_core_after,
        residency_before,
        residency_after,
    }))
}

#[cfg(test)]
mod tests {
    use crate::mincore::PageMapSlice as _;

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

                type R<O> = FileResult<O, $t>;

                #[test]
                fn query_counts_pages() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: R<()> = process_file(&Query, f.path(), &range, false)
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
                    let result: Option<R<()>> =
                        process_file(&Query, f.path(), &range, false).unwrap();
                    assert!(result.is_none());
                }

                #[test]
                fn offset_beyond_file() {
                    let (f, _) = create_temp_file(1);
                    let range = FileRange {
                        offset: 1_000_000,
                        max_len: None,
                    };
                    let result: crate::Result<Option<R<()>>> =
                        process_file(&Query, f.path(), &range, false);
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
                    let result: R<()> = process_file(&Query, f.path(), &range, false)
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

                    process_file::<_, $t>(&Evict, f.path(), &range, false).unwrap();
                    process_file::<_, $t>(&Touch, f.path(), &range, false).unwrap();

                    let file = File::open(f.path()).unwrap();
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
                    let result: crate::Result<Option<R<()>>> =
                        process_file(&Evict, f.path(), &range, false);
                    assert!(result.is_ok());
                }

                #[test]
                fn with_residency() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: R<()> = process_file(&Query, f.path(), &range, true)
                        .unwrap()
                        .unwrap();
                    assert!(result.residency_before.is_some());
                    assert!(result.residency_after.is_some());
                    assert_eq!(result.residency_before.unwrap().len(), 4);
                }

                #[test]
                fn without_residency() {
                    let (f, _) = create_temp_file(4);
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: R<()> = process_file(&Query, f.path(), &range, false)
                        .unwrap()
                        .unwrap();
                    if *crate::cachestat::SUPPORTED {
                        assert!(result.residency_before.is_none());
                        assert!(result.residency_after.is_none());
                    }
                }

                #[test]
                fn nonexistent_returns_error() {
                    let range = FileRange {
                        offset: 0,
                        max_len: None,
                    };
                    let result: crate::Result<Option<R<()>>> = process_file(
                        &Query,
                        std::path::Path::new("/nonexistent/file.dat"),
                        &range,
                        false,
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
        assert_eq!(stats.total_pages_in_core.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_files.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_dirs.load(Ordering::Relaxed), 0);
    }
}
