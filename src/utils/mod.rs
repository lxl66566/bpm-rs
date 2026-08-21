pub mod path;
pub mod table;
pub mod url;

pub use path::FileNameExt;
pub use url::UrlJoinAll;

#[cfg(unix)]
pub fn is_root() -> bool {
    unsafe { libc::getuid() == 0 }
}
