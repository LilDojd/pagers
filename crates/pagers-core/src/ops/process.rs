use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use bitvec::vec::BitVec;
use memmap2::MmapOptions;

use super::{FileContext, FileInfo, FileRange, FileResult, Op};
use crate::Error;

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

fn page_count(
    file: &File,
    mmap: &memmap2::Mmap,
    offset: u64,
    len: usize,
) -> crate::Result<(i64, Option<BitVec>)> {
    if *crate::cachestat::SUPPORTED {
        use std::os::unix::io::AsFd;
        let count = crate::cachestat::cached_pages(file.as_fd(), offset, len as u64)? as i64;
        Ok((count, None))
    } else {
        let residency: BitVec = crate::mincore::residency(mmap, len)?;
        let count = residency.count_ones() as i64;
        Ok((count, Some(residency)))
    }
}

fn page_count_with_residency(mmap: &memmap2::Mmap, len: usize) -> crate::Result<(i64, BitVec)> {
    let residency: BitVec = crate::mincore::residency(mmap, len)?;
    let count = residency.count_ones() as i64;
    Ok((count, residency))
}

pub fn file_info(path: &Path, range: &FileRange) -> crate::Result<Option<FileInfo>> {
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
    let residency: BitVec = crate::mincore::residency(&mmap, len)?;
    Ok(Some(FileInfo {
        total_pages,
        residency,
    }))
}

pub fn process_file<O: Op>(
    op: &O,
    path: &Path,
    range: &FileRange,
    with_residency: bool,
) -> crate::Result<Option<FileResult<O::Output>>> {
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
        let (count, bv) = page_count_with_residency(&mmap, len)?;
        (count, Some(bv))
    } else {
        let (count, _) = page_count(&file, &mmap, offset, len)?;
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
        let (count, bv) = page_count_with_residency(&mmap, len)?;
        (count, Some(bv))
    } else {
        let (count, _) = page_count(&file, &mmap, offset, len)?;
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

    #[test]
    fn test_process_file_query_counts_pages() {
        let (f, _) = create_temp_file(4);
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(&Query, f.path(), &range, false)
            .unwrap()
            .unwrap();
        assert_eq!(result.total_pages, 4);
    }

    #[test]
    fn test_process_file_empty_file_returns_none() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(&Query, f.path(), &range, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_process_file_offset_beyond_file() {
        let (f, _) = create_temp_file(1);
        let range = FileRange {
            offset: 1_000_000,
            max_len: None,
        };
        let result = process_file(&Query, f.path(), &range, false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, Error::OffsetBeyondFile { .. }),
            "expected OffsetBeyondFile, got: {err}"
        );
    }

    #[test]
    fn test_process_file_with_max_len() {
        let (f, _) = create_temp_file(8);
        let page_size = *crate::pagesize::PAGE_SIZE;
        let range = FileRange {
            offset: 0,
            max_len: Some((page_size * 2) as u64),
        };
        let result = process_file(&Query, f.path(), &range, false)
            .unwrap()
            .unwrap();
        assert_eq!(result.total_pages, 2);
    }

    #[test]
    fn test_process_file_touch_makes_resident() {
        let (f, size) = create_temp_file(4);
        let range = FileRange {
            offset: 0,
            max_len: None,
        };

        process_file(&Evict, f.path(), &range, false).unwrap();
        process_file(&Touch, f.path(), &range, false).unwrap();

        let file = File::open(f.path()).unwrap();
        let mmap_check = unsafe { memmap2::MmapOptions::new().len(size).map(&file).unwrap() };
        let residency: BitVec = crate::mincore::residency(&mmap_check, size).unwrap();
        assert!(residency.all(), "expected all pages resident after touch");
    }

    #[test]
    fn test_process_file_evict_succeeds() {
        let (f, _) = create_temp_file(4);
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(&Evict, f.path(), &range, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_file_with_residency() {
        let (f, _) = create_temp_file(4);
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(&Query, f.path(), &range, true)
            .unwrap()
            .unwrap();
        assert!(result.residency_before.is_some());
        assert!(result.residency_after.is_some());
        assert_eq!(result.residency_before.unwrap().len(), 4);
    }

    #[test]
    fn test_process_file_without_residency() {
        let (f, _) = create_temp_file(4);
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(&Query, f.path(), &range, false)
            .unwrap()
            .unwrap();
        if *crate::cachestat::SUPPORTED {
            assert!(result.residency_before.is_none());
            assert!(result.residency_after.is_none());
        }
    }

    #[test]
    fn test_process_file_nonexistent_returns_error() {
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let result = process_file(
            &Query,
            std::path::Path::new("/nonexistent/file.dat"),
            &range,
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_file_info() {
        let (f, _) = create_temp_file(4);
        let range = FileRange {
            offset: 0,
            max_len: None,
        };
        let info = file_info(f.path(), &range).unwrap().unwrap();
        assert_eq!(info.total_pages, 4);
        assert_eq!(info.residency.len(), 4);
    }

    #[test]
    fn test_stats_default() {
        let stats = Stats::default();
        assert_eq!(stats.total_pages.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_pages_in_core.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_files.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_dirs.load(Ordering::Relaxed), 0);
    }
}
