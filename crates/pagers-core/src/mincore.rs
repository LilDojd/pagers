use memmap2::Mmap;
use nix::errno::Errno;

pub fn residency<T>(mmap: &Mmap, len: usize) -> nix::Result<T>
where
    T: std::iter::FromIterator<bool>,
{
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
            vec_out.as_mut_ptr(),
        ) != 0
        {
            return Err(Errno::last());
        }
        // SAFETY: we just filled up the vector with valid values
        vec_out.set_len(vec_len);
    }
    Ok(vec_out.into_iter().map(|x| x != 0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitvec::vec::BitVec;
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
            }
        };
    }

    residency_tests!(Vec<bool>, vec_bool);
    residency_tests!(BitVec, bitvec);
}
