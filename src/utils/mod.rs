pub mod constants;
pub mod err;

use std::path::{Path, PathBuf};

/// print red message and exit with return code 1.
#[allow(unused_imports)]
#[macro_export]
macro_rules! error_exit  {
    ($fmt:expr $(, $arg:expr)*) => {
        {
            eprintln!("\x1b[31m{}: {}\x1b[0m", "Error", format!($fmt $(, $arg)*));
            std::process::exit(1);
        }
    };
}
pub use error_exit;

#[allow(unused_imports)]
#[macro_export]
macro_rules! assert_exit  {
    ($cond:expr $(, $arg:expr)*) => {
        if !$cond
        {
            error_exit!($($arg),*);
        }
    };
}
pub use assert_exit;

/// join given strs.
pub fn path_join(paths: impl IntoIterator<Item = impl AsRef<Path>>) -> PathBuf {
    paths.into_iter().fold(PathBuf::new(), |acc, p| acc.join(p))
}

pub fn fmt_repo_list(name: &str, url: &str, version: &str) -> String {
    format!("{:20}{:50}{:20}", name, url, version)
}
