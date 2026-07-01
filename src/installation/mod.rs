pub mod download;
pub mod unzip;

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Check if a directory contains exactly one entry, returning its path.
#[inline]
pub fn only_one_file_in_dir(path: impl AsRef<Path>) -> std::io::Result<Option<PathBuf>> {
    let mut iter = fs::read_dir(path)?;
    match (iter.next(), iter.next()) {
        (Some(Ok(entry)), None) => Ok(Some(entry.path())),
        _ => Ok(None),
    }
}

/// Platform-specific installation logic.
pub trait Installation {
    fn install(
        &mut self,
        src: impl AsRef<Path>,
        ctx: &crate::context::Context,
    ) -> anyhow::Result<()>;
    fn uninstall(&mut self, ctx: &crate::context::Context) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_only_one_file_in_dir() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path();

        // 1. 空文件夹：应返回 None
        assert_eq!(only_one_file_in_dir(dir_path).unwrap(), None);

        // 2. 只有一个文件：应返回 Some(path)
        let file1 = dir_path.join("file1.txt");
        fs::write(&file1, "dummy data").unwrap();
        assert_eq!(only_one_file_in_dir(dir_path).unwrap(), Some(file1.clone()));

        // 3. 有两个文件：应返回 None
        let file2 = dir_path.join("file2.txt");
        fs::write(&file2, "dummy data").unwrap();
        assert_eq!(only_one_file_in_dir(dir_path).unwrap(), None);
    }
}
