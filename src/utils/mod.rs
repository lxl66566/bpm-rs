pub mod log;
pub mod path;
pub mod table;
pub mod url;

pub use log::log_init;
pub use url::*;

#[cfg(unix)]
pub fn is_root() -> bool {
    unsafe { libc::getuid() == 0 }
}
