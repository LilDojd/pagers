use clap_verbosity_flag::Verbosity;

pub fn init(verbosity: &Verbosity) {
    let filter = verbosity.tracing_level_filter();
    if filter == tracing::level_filters::LevelFilter::OFF {
        return;
    }
    tracing_subscriber::fmt()
        .with_max_level(filter)
        .with_writer(std::io::stderr)
        .init();
}
