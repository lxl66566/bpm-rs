pub mod path;
pub mod table;
pub mod url;

pub use path::FileNameExt;
pub use url::*;

#[cfg(unix)]
pub fn is_root() -> bool {
    unsafe { libc::getuid() == 0 }
}

#[cfg(unix)]
pub fn check_root() -> anyhow::Result<()> {
    if !is_root() {
        anyhow::bail!("You must run as root to perform this operation.");
    }
    Ok(())
}
