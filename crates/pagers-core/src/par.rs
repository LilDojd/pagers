pub(crate) trait SeenInodes {
    fn already_seen(&self, key: (u64, u64)) -> bool;
}

#[cfg(feature = "rayon")]
impl SeenInodes for dashmap::DashMap<(u64, u64), ()> {
    fn already_seen(&self, key: (u64, u64)) -> bool {
        self.insert(key, ()).is_some()
    }
}

#[cfg(not(feature = "rayon"))]
impl SeenInodes for std::cell::RefCell<std::collections::HashSet<(u64, u64)>> {
    fn already_seen(&self, key: (u64, u64)) -> bool {
        !self.borrow_mut().insert(key)
    }
}

#[cfg(feature = "rayon")]
pub(crate) type InodeSet = dashmap::DashMap<(u64, u64), ()>;
#[cfg(not(feature = "rayon"))]
pub(crate) type InodeSet = std::cell::RefCell<std::collections::HashSet<(u64, u64)>>;

pub(crate) fn par_collect<T: Sync, R: Send>(
    items: &[T],
    f: impl Fn(&T) -> Option<R> + Sync,
) -> Vec<R> {
    #[cfg(feature = "rayon")]
    if items.len() > 1 {
        use rayon::prelude::*;
        let threads = items
            .len()
            .min(std::thread::available_parallelism().map_or(4, |n| n.get()));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .use_current_thread()
            .build()
            .unwrap();
        return pool.install(|| items.par_iter().filter_map(|i| f(i)).collect());
    }
    items.iter().filter_map(|i| f(i)).collect()
}
