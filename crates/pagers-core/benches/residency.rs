use std::io::Write;

use criterion::{Criterion, criterion_group, criterion_main};
use memmap2::MmapOptions;
use pagers_core::mincore::{self, PageMap, PageMapSlice};

fn create_mmap(pages: usize) -> (tempfile::NamedTempFile, memmap2::Mmap, usize) {
    let page_size = *pagers_core::pagesize::PAGE_SIZE;
    let size = page_size * pages;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(&vec![0xABu8; size]).unwrap();
    f.flush().unwrap();
    let mmap = unsafe { MmapOptions::new().map(f.as_file()).unwrap() };
    (f, mmap, size)
}

fn bench_residency(c: &mut Criterion) {
    let page_counts = [1, 1024, 65536, 655360];

    let mut group = c.benchmark_group("residency");
    for &pages in &page_counts {
        let (_f, mmap, size) = create_mmap(pages);

        group.bench_with_input(format!("Vec<bool>/{pages}"), &pages, |b, _| {
            b.iter(|| {
                let r: Vec<bool> = mincore::residency(&mmap, size).unwrap();
                assert!(!PageMapSlice::is_empty(&r));
            });
        });

        #[cfg(feature = "bitvec")]
        group.bench_with_input(format!("BitVec/{pages}"), &pages, |b, _| {
            b.iter(|| {
                let r: bitvec::vec::BitVec = mincore::residency(&mmap, size).unwrap();
                assert!(!PageMapSlice::is_empty(&r));
            });
        });
    }
    group.finish();
}

fn bench_count_filled(c: &mut Criterion) {
    let page_counts = [1024, 65536, 655360];

    let mut group = c.benchmark_group("count_filled");
    for &pages in &page_counts {
        let (_f, mmap, size) = create_mmap(pages);

        let vec_pm: Vec<bool> = mincore::residency(&mmap, size).unwrap();
        group.bench_with_input(format!("Vec<bool>/{pages}"), &pages, |b, _| {
            b.iter(|| PageMapSlice::count_filled(&vec_pm));
        });

        #[cfg(feature = "bitvec")]
        {
            let bit_pm: bitvec::vec::BitVec = mincore::residency(&mmap, size).unwrap();
            group.bench_with_input(format!("BitVec/{pages}"), &pages, |b, _| {
                b.iter(|| PageMapSlice::count_filled(&bit_pm));
            });
        }
    }
    group.finish();
}

fn bench_from_bools(c: &mut Criterion) {
    let page_counts = [1024, 65536, 655360];

    let mut group = c.benchmark_group("from_bools");
    for &pages in &page_counts {
        let bools: Vec<bool> = (0..pages).map(|i| i % 3 == 0).collect();

        group.bench_with_input(format!("Vec<bool>/{pages}"), &pages, |b, _| {
            b.iter(|| Vec::<bool>::from_bools(bools.iter().copied()));
        });

        #[cfg(feature = "bitvec")]
        group.bench_with_input(format!("BitVec/{pages}"), &pages, |b, _| {
            b.iter(|| bitvec::vec::BitVec::from_bools(bools.iter().copied()));
        });
    }
    group.finish();
}

fn bench_from_residency_bytes(c: &mut Criterion) {
    let page_counts = [1024, 65536, 655360];

    let mut group = c.benchmark_group("from_residency_bytes");
    for &pages in &page_counts {
        let bytes: Vec<u8> = (0..pages).map(|i| if i % 3 == 0 { 1 } else { 0 }).collect();

        group.bench_with_input(format!("Vec<bool>/{pages}"), &pages, |b, _| {
            b.iter(|| Vec::<bool>::from_residency_bytes(bytes.clone()));
        });

        #[cfg(feature = "bitvec")]
        group.bench_with_input(format!("BitVec/{pages}"), &pages, |b, _| {
            b.iter(|| bitvec::vec::BitVec::from_residency_bytes(bytes.clone()));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_residency,
    bench_count_filled,
    bench_from_bools,
    bench_from_residency_bytes
);
criterion_main!(benches);
