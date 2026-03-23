lazy_static::lazy_static! {
    pub static ref PAGE_SIZE: usize = {
        usize::try_from(
            nix::unistd::sysconf(nix::unistd::SysconfVar::PAGE_SIZE)
                .expect("Failed to fetch _SC_PAGESIZE")
                .expect("_SC_PAGESIZE returned None"),
        )
        .unwrap()
    };
}
