use log::LevelFilter;

#[inline]
pub fn log_init() {
    log_init_with_default_level(LevelFilter::Info);
}

#[inline]
pub fn log_init_with_default_level(level: LevelFilter) {
    _ = pretty_env_logger::formatted_builder()
        .filter_level(level)
        .format_timestamp_millis()
        .filter_module("j4rs", LevelFilter::Info)
        .parse_default_env()
        .try_init();
}
