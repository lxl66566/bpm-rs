use std::{
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::Result;
use compress_tools::{uncompress_archive, Ownership};
use log::debug;

use crate::{installation::only_one_file_in_dir, utils::path::PathExt};

/// unzip the given archive, and remove the archive.
///
/// # Returns
///
/// returns the `main` path of unzipped archive path.
pub fn unzip(src: impl Into<PathBuf>, to: impl AsRef<Path>) -> Result<PathBuf> {
    let to = to.as_ref();
    let src = src.into();
    // if `to` does not exist, libarchive will create it.
    let mut source = File::open(&src)?;
    uncompress_archive(&mut source, to, Ownership::Preserve)?;
    assert!(to.is_dir());
    std::fs::remove_file(&src)?;
    if let Some(folder) = only_one_file_in_dir(to)? {
        debug!(
            "unwrap archive folder: {} -> {}",
            to.display(),
            folder.display()
        );
        if folder.is_dir() {
            return Ok(folder);
        }
    }
    Ok(to.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unzip() -> Result<()> {
        let assets_dir = PathBuf::from("test_assets");
        let tempdir = tempfile::tempdir()?;
        let another_temp = tempfile::tempdir()?;
        for p in ["noroot.zip", "noroot.tar.gz", "root.tar.gz"] {
            let true_src = assets_dir.join(p);
            let src = another_temp.path().join(p);
            // Because `unzip` will remove the archive file, so we need to copy before
            // testing.
            std::fs::copy(true_src, &src)?;
            let to = tempdir.path().join(p);
            let main = unzip(src, &to)?;
            if p.starts_with("root") {
                assert_eq!(main, to.join("root"));
            } else {
                assert_eq!(main, to);
            }
        }
        Ok(())
    }
}
