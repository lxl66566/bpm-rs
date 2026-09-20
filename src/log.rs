use std::{io::IsTerminal, sync::LazyLock};

use colored::Colorize;
use flexi_logger::{DeferredNow, LogSpecification, Logger, Record};
use log::LevelFilter;

/// `colored` auto-detects the *stdout* TTY, but flexi_logger writes to
/// stderr: decide by the stream we actually write to, or redirected logs
/// would still carry ANSI codes.
static STDERR_IS_TTY: LazyLock<bool> = LazyLock::new(|| std::io::stderr().is_terminal());

#[inline]
pub fn log_init() {
    #[cfg(not(debug_assertions))]
    log_init_with_default_level(LevelFilter::Info);
    #[cfg(debug_assertions)]
    log_init_with_default_level(LevelFilter::Debug);
}

#[inline]
pub fn log_init_with_default_level(level: LevelFilter) {
    // `reqwest=warn`, not `=info`: equal to the global level it silenced
    // nothing; warn is what hides the per-request INFO noise.
    let default = match level {
        LevelFilter::Off => "off",
        LevelFilter::Error => "error",
        LevelFilter::Warn => "warn",
        LevelFilter::Info => "info, reqwest=warn",
        LevelFilter::Debug => "debug, reqwest=warn",
        LevelFilter::Trace => "trace, reqwest=warn",
    };

    // RUST_LOG takes priority when set: release builds have no other way to
    // raise the level for troubleshooting. The reqwest override is not
    // appended to a user spec — the user's directives are authoritative (and
    // they can silence reqwest themselves). A malformed RUST_LOG falls back
    // to the built-in spec with a note instead of aborting startup.
    let spec = match std::env::var("RUST_LOG") {
        Ok(user) => LogSpecification::parse(&user).unwrap_or_else(|e| {
            eprintln!("bpm: ignoring invalid RUST_LOG `{user}` ({e}); using `{default}`");
            LogSpecification::parse(default).expect("built-in log spec is valid")
        }),
        Err(_) => LogSpecification::parse(default).expect("built-in log spec is valid"),
    };

    Logger::with(spec)
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
    write!(buf, "{level:5} {}", record.args())
}

fn colored_level(level: log::Level) -> colored::ColoredString {
    if !*STDERR_IS_TTY {
        // Plain (uncolored) ColoredString: no ANSI codes in redirected logs.
        // (as_str: String::clear is the inherent Vec::clear, not Colorize's.)
        return format!("{level:5}").as_str().clear();
    }
    match level {
        log::Level::Error => format!("{:5}", "ERROR").red(),
        log::Level::Warn => format!("{:5}", "WARN").yellow(),
        log::Level::Info => format!("{:5}", "INFO").green(),
        log::Level::Debug => format!("{:5}", "DEBUG").blue(),
        log::Level::Trace => format!("{:5}", "TRACE").magenta(),
    }
}
