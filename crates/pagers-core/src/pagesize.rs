use std::sync::LazyLock;

pub static PAGE_SIZE: LazyLock<usize> = LazyLock::new(|| {
    usize::try_from(
        nix::unistd::sysconf(nix::unistd::SysconfVar::PAGE_SIZE)
            .expect("Failed to fetch _SC_PAGESIZE")
            .expect("_SC_PAGESIZE returned None"),
    )
    .unwrap()
});
