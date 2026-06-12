use colored::Colorize;
use flexi_logger::{DeferredNow, Logger, Record};
use log::LevelFilter;

#[inline]
pub fn log_init() {
    #[cfg(not(debug_assertions))]
    log_init_with_default_level(LevelFilter::Info);
    #[cfg(debug_assertions)]
    log_init_with_default_level(LevelFilter::Debug);
}

#[inline]
pub fn log_init_with_default_level(level: LevelFilter) {
    let spec = match level {
        LevelFilter::Off => "off".to_string(),
        LevelFilter::Error => "error".to_string(),
        LevelFilter::Warn => "warn".to_string(),
        LevelFilter::Info => "info, reqwest=info".to_string(),
        LevelFilter::Debug => "debug, reqwest=info".to_string(),
        LevelFilter::Trace => "trace, reqwest=info".to_string(),
    };

    Logger::try_with_str(spec)
        .unwrap()
        .format_for_stderr(log_format)
        .start()
        .unwrap_or_else(|e| panic!("Logger initialization failed: {e}"));
}

fn log_format(
    buf: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record<'_>,
) -> std::io::Result<()> {
    let level = colored_level(record.level());
    writeln!(buf, "{level:5} {}", record.args())
}

fn colored_level(level: log::Level) -> colored::ColoredString {
    match level {
        log::Level::Error => format!("{:5}", "ERROR").red(),
        log::Level::Warn => format!("{:5}", "WARN").yellow(),
        log::Level::Info => format!("{:5}", "INFO").green(),
        log::Level::Debug => format!("{:5}", "DEBUG").blue(),
        log::Level::Trace => format!("{:5}", "TRACE").magenta(),
    }
}

#[inline]
pub fn set_quiet_log() {
    log::set_max_level(log::LevelFilter::Warn);
}
