use std::ops::{Index, IndexMut, Range};

use memmap2::Mmap;
use nix::errno::Errno;

#[cfg(target_os = "linux")]
type MincoreMutVecRef = *mut u8;
#[cfg(target_os = "macos")]
type MincoreMutVecRef = *mut i8;

pub fn residency<PM: PageMap>(mmap: &Mmap, len: usize) -> nix::Result<PM> {
    let page_size = *crate::pagesize::PAGE_SIZE;
    let vec_len = len.div_ceil(page_size);
    let mut vec_out: Vec<u8> = Vec::with_capacity(vec_len);

    unsafe {
        // SAFETY: mincore takes a pointer to a virtual memory region and writes
        // RAM residency information to the memory region at vec_out, with the
        // length computed above using the expression from the mincore man page.
        // We have allocated the underlying buffer by using with_capacity.
        if libc::mincore(
            mmap.as_ptr() as *mut libc::c_void,
            len,
            vec_out.as_mut_ptr() as MincoreMutVecRef,
        ) != 0
        {
            return Err(Errno::last());
        }
        // SAFETY: we just filled up the vector with valid values
        vec_out.set_len(vec_len);
    }
    Ok(PM::from_residency_bytes(vec_out))
}

pub trait PageMapSlice {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn count_filled(&self) -> usize;
    fn fill(&mut self, value: bool);
}

pub trait PageMap:
    PageMapSlice + Index<Range<usize>, Output = Self::Slice> + IndexMut<Range<usize>>
{
    type Slice: ?Sized + PageMapSlice;

    fn from_bools(iter: impl Iterator<Item = bool>) -> Self;

    fn from_residency_bytes(bytes: Vec<u8>) -> Self
    where
        Self: Sized,
    {
        Self::from_bools(bytes.into_iter().map(|b| b != 0))
    }
}

#[cfg(feature = "bitvec")]
pub type DefaultPageMap = bitvec::vec::BitVec;

#[cfg(not(feature = "bitvec"))]
pub type DefaultPageMap = Vec<bool>;

// Vec<bool> / [bool] impls

impl PageMapSlice for [bool] {
    fn len(&self) -> usize {
        self.len()
    }

    fn count_filled(&self) -> usize {
        self.iter().filter(|&&v| v).count()
    }

    fn fill(&mut self, value: bool) {
        self.fill(value);
    }
}

impl PageMapSlice for Vec<bool> {
    fn len(&self) -> usize {
        self.len()
    }

    fn count_filled(&self) -> usize {
        self.iter().filter(|&&v| v).count()
    }

    fn fill(&mut self, value: bool) {
        self.as_mut_slice().fill(value);
    }
}

impl PageMap for Vec<bool> {
    type Slice = [bool];

    fn from_bools(iter: impl Iterator<Item = bool>) -> Self {
        iter.collect()
    }
}

// BitVec / BitSlice impls

#[cfg(feature = "bitvec")]
mod bitvec_impl {
    use super::*;
    use bitvec::prelude::*;

    impl PageMapSlice for BitSlice {
        fn len(&self) -> usize {
            self.len()
        }

        fn count_filled(&self) -> usize {
            self.count_ones()
        }

        fn fill(&mut self, value: bool) {
            self.fill(value);
        }
    }

    impl PageMapSlice for BitVec {
        fn len(&self) -> usize {
            self.len()
        }

        fn count_filled(&self) -> usize {
            self.count_ones()
        }

        fn fill(&mut self, value: bool) {
            self.as_mut_bitslice().fill(value);
        }
    }

    impl PageMap for BitVec {
        type Slice = BitSlice;

        fn from_bools(iter: impl Iterator<Item = bool>) -> Self {
            iter.collect()
        }

        fn from_residency_bytes(bytes: Vec<u8>) -> Self {
            let len = bytes.len();
            let bits_per_word = usize::BITS as usize;
            let packed: Vec<usize> = bytes
                .chunks(bits_per_word)
                .map(|chunk| {
                    chunk
                        .iter()
                        .enumerate()
                        .fold(0usize, |acc, (i, &b)| acc | (((b != 0) as usize) << i))
                })
                .collect();
            let mut bv = BitVec::from_vec(packed);
            bv.truncate(len);
            bv
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memmap2::MmapOptions;
    use std::io::Write;

    fn create_temp_file(size: usize) -> (tempfile::NamedTempFile, Mmap) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let data = vec![0xABu8; size];
        f.write_all(&data).unwrap();
        f.flush().unwrap();

        let mmap = unsafe { MmapOptions::new().map(f.as_file()).unwrap() };
        (f, mmap)
    }

    macro_rules! residency_tests {
        ($t:ty, $mod:ident) => {
            mod $mod {
                use super::*;

                #[test]
                fn page_count() {
                    let ps = *crate::pagesize::PAGE_SIZE;
                    let size = ps * 4;
                    let (_f, mmap) = create_temp_file(size);
                    let res: $t = residency(&mmap, size).unwrap();
                    assert_eq!(res.len(), 4);
                }

                #[test]
                fn after_touch_all_resident() {
                    let ps = *crate::pagesize::PAGE_SIZE;
                    let size = ps * 4;
                    let (_f, mmap) = create_temp_file(size);

                    let mut junk: u8 = 0;
                    for i in 0..4 {
                        junk = junk.wrapping_add(mmap[i * ps]);
                    }
                    let _ = junk;

                    let res: $t = residency(&mmap, size).unwrap();
                    assert!(res.iter().all(|r| *r));
                }

                #[test]
                fn partial_page_rounds_up() {
                    let ps = *crate::pagesize::PAGE_SIZE;
                    let size = ps * 3 + 1;
                    let (_f, mmap) = create_temp_file(size);
                    let res: $t = residency(&mmap, size).unwrap();
                    assert_eq!(res.len(), 4);
                }

                #[test]
                fn single_page() {
                    let ps = *crate::pagesize::PAGE_SIZE;
                    let (_f, mmap) = create_temp_file(ps);
                    let _ = mmap[0];
                    let res: $t = residency(&mmap, ps).unwrap();
                    assert_eq!(res.len(), 1);
                    assert_eq!(PageMapSlice::count_filled(&res), 1);
                }

                #[test]
                fn count_filled_after_touch() {
                    let ps = *crate::pagesize::PAGE_SIZE;
                    let size = ps * 4;
                    let (_f, mmap) = create_temp_file(size);
                    let _ = mmap[0];
                    let _ = mmap[ps * 2];
                    let res: $t = residency(&mmap, size).unwrap();
                    assert!(PageMapSlice::count_filled(&res) >= 2);
                }

                #[test]
                fn slice_range() {
                    let ps = *crate::pagesize::PAGE_SIZE;
                    let size = ps * 4;
                    let (_f, mmap) = create_temp_file(size);
                    let res: $t = residency(&mmap, size).unwrap();
                    let slice = &res[1..3];
                    assert_eq!(PageMapSlice::len(slice), 2);
                }

                #[test]
                fn fill_sets_all() {
                    let mut pm = <$t>::from_bools([true, false, true, false].into_iter());
                    PageMapSlice::fill(&mut pm, true);
                    assert_eq!(PageMapSlice::count_filled(&pm), 4);
                    PageMapSlice::fill(&mut pm, false);
                    assert_eq!(PageMapSlice::count_filled(&pm), 0);
                }

                #[test]
                fn from_bools_empty() {
                    let pm = <$t>::from_bools(std::iter::empty());
                    assert!(PageMapSlice::is_empty(&pm));
                }
            }
        };
    }

    residency_tests!(Vec<bool>, vec_bool_impl);

    #[cfg(feature = "bitvec")]
    residency_tests!(::bitvec::vec::BitVec, bitvec_impl);
}
