use clap_verbosity_flag::Verbosity;

pub fn init(verbosity: &Verbosity) {
    tracing_subscriber::fmt()
        .with_max_level(verbosity.tracing_level_filter())
        .with_writer(std::io::stderr)
        .init();
}
