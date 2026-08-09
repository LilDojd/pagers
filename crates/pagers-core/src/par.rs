pub(crate) trait SeenInodes {
    fn already_seen(&self, key: (u64, u64)) -> bool;
}

#[cfg(feature = "rayon")]
mod rayon_impl {
    use std::num::NonZeroU16;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub enum Threads {
        #[default]
        All,
        Exact(NonZeroU16),
    }

    impl From<u16> for Threads {
        fn from(n: u16) -> Self {
            match NonZeroU16::new(n) {
                None => Self::All,
                Some(n) => Self::Exact(n),
            }
        }
    }

    impl std::fmt::Display for Threads {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::All => f.write_str("0"),
                Self::Exact(n) => write!(f, "{n}"),
            }
        }
    }

    impl std::str::FromStr for Threads {
        type Err = std::num::ParseIntError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let n: u16 = s.parse()?;
            Ok(Self::from(n))
        }
    }

    impl Threads {
        pub fn num_threads(self) -> usize {
            match self {
                Self::All => 0,
                Self::Exact(n) => n.get() as usize,
            }
        }
    }

    impl super::SeenInodes for dashmap::DashMap<(u64, u64), ()> {
        fn already_seen(&self, key: (u64, u64)) -> bool {
            self.insert(key, ()).is_some()
        }
    }

    pub(crate) type InodeSet = dashmap::DashMap<(u64, u64), ()>;
}

#[cfg(feature = "rayon")]
pub(crate) use rayon_impl::InodeSet;
#[cfg(feature = "rayon")]
pub use rayon_impl::Threads;

#[cfg(not(feature = "rayon"))]
impl SeenInodes for std::cell::RefCell<std::collections::HashSet<(u64, u64)>> {
    fn already_seen(&self, key: (u64, u64)) -> bool {
        !self.borrow_mut().insert(key)
    }
}

#[cfg(not(feature = "rayon"))]
pub(crate) type InodeSet = std::cell::RefCell<std::collections::HashSet<(u64, u64)>>;

#[cfg(test)]
#[cfg(feature = "rayon")]
mod tests {
    use std::num::NonZeroU16;

    use super::*;

    fn exact(n: u16) -> Threads {
        Threads::Exact(NonZeroU16::new(n).unwrap())
    }

    #[test]
    fn threads_from_zero_is_all() {
        assert_eq!(Threads::from(0), Threads::All);
    }

    #[test]
    fn threads_from_nonzero_is_exact() {
        assert_eq!(Threads::from(4), exact(4));
        assert_eq!(Threads::from(1), exact(1));
    }

    #[test]
    fn threads_default_is_all() {
        assert_eq!(Threads::default(), Threads::All);
    }

    #[test]
    fn threads_num_threads_all_is_zero() {
        assert_eq!(Threads::All.num_threads(), 0);
    }

    #[test]
    fn threads_num_threads_exact() {
        assert_eq!(exact(8).num_threads(), 8);
        assert_eq!(exact(1).num_threads(), 1);
    }

    #[test]
    fn threads_display() {
        assert_eq!(Threads::All.to_string(), "0");
        assert_eq!(exact(4).to_string(), "4");
    }

    #[test]
    fn threads_from_str() {
        assert_eq!("0".parse::<Threads>(), Ok(Threads::All));
        assert_eq!("4".parse::<Threads>(), Ok(exact(4)));
        assert_eq!("1".parse::<Threads>(), Ok(exact(1)));
        assert!("abc".parse::<Threads>().is_err());
    }

    #[test]
    fn threads_display_roundtrip() {
        for t in [Threads::All, exact(1), exact(8)] {
            assert_eq!(t.to_string().parse::<Threads>(), Ok(t));
        }
    }
}
