pub mod path;
pub mod table;
pub mod url;

use std::sync::atomic::{AtomicBool, Ordering};

pub use path::FileNameExt;
pub use url::UrlJoinAll;

#[cfg(unix)]
pub fn is_root() -> bool {
    unsafe { libc::getuid() == 0 }
}

static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Whether graceful cancellation (Ctrl-C) has been requested.
pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

/// Register a Ctrl-C handler that requests graceful cancellation; long
/// operations check [`cancelled()`] between safe points and unwind through
/// their normal error path, so rollback handlers still run. A second Ctrl-C
/// force-quits for impatient users.
pub fn spawn_cancel_handler() {
    tokio::spawn(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            CANCELLED.store(true, Ordering::Relaxed);
            log::warn!("Received Ctrl-C, cancelling... Press Ctrl-C again to force quit.");
            let _ = tokio::signal::ctrl_c().await;
            std::process::exit(130);
        }
    });
}
