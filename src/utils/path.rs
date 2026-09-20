#[cfg(windows)]
use std::path::{Component, Prefix};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use walkdir::WalkDir;

use crate::installation::unzip::TAR_EXTENSIONS;

pub trait PathExt {
    fn glob_name(&self, pattern: &str) -> Vec<PathBuf>;
    /// create dir if not exist
    ///
    /// # Errors
    ///
    /// - `Ok(())` if dir already exists
    /// - `Err(e)` otherwise
    fn create_dir_if_not_exist(&self) -> Result<(), std::io::Error>;
    /// remove dir or file, do not throw error if not exist
    ///
    /// # Errors
    ///
    /// - `Ok(())` if dir missing or remove successfully
    /// - `Err(e)` otherwise
    fn remove_all_allow_missing(&self) -> Result<(), std::io::Error>;
}

impl<P: AsRef<Path>> PathExt for P {
    fn glob_name(&self, pattern: &str) -> Vec<PathBuf> {
        // Use walkdir instead of hand-rolled recursion (stack-overflow safe,
        // cleaner filtering).
        WalkDir::new(self)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file()) // keep files only
            .filter(|e| e.file_name() == pattern) // match by file name
            .map(|e| e.path().to_path_buf())
            .collect()
    }

    fn create_dir_if_not_exist(&self) -> Result<(), std::io::Error> {
        // fs::create_dir_all is already a no-op when the dir exists, so no
        // manual existence check is needed.
        fs::create_dir_all(self)
    }

    fn remove_all_allow_missing(&self) -> Result<(), std::io::Error> {
        let path = self.as_ref();

        // Check the dir case first (is_dir returns false when missing).
        if path.is_dir() {
            match fs::remove_dir_all(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            // A file, or a path that does not exist at all.
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        }
    }
}

/// Extract the drive letter and the `/`-joined relative part of a path.
///
/// Returns `None` when the path has no disk prefix (UNC/network or relative
/// paths); callers turn that into an error instead of a panic, since a domain
/// home dir can legitimately be a UNC path.
#[cfg(windows)]
fn extract_windows_drive_and_relative(path: &Path) -> Option<(char, String)> {
    let mut components = path.components();

    // Extract the drive letter.
    let drive = match components.next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(d) | Prefix::VerbatimDisk(d) => (d as char).to_ascii_lowercase(),
            // UNC and device prefixes have no drive letter.
            _ => return None,
        },
        _ => return None,
    };

    // Collect the rest (the RootDir component of `C:\` is filtered out).
    let relative: Vec<String> = components
        .filter(|c| !matches!(c, Component::RootDir))
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    Some((drive, relative.join("/")))
}

#[cfg(windows)]
fn not_local_disk_error(p: &Path) -> anyhow::Error {
    anyhow!(
        "`{}` is not an absolute local-disk path (e.g. `C:\\...`); UNC/network paths are not \
         supported",
        p.display()
    )
}

/// Build the posix path with the given mount prefix (`""` for git-bash,
/// `"/mnt"` for WSL).
#[cfg(windows)]
fn windows_path_to_posix(p: &Path, mount_prefix: &str) -> Result<String> {
    let (drive, relative) =
        extract_windows_drive_and_relative(p).ok_or_else(|| not_local_disk_error(p))?;
    if relative.is_empty() {
        Ok(format!("{mount_prefix}/{drive}"))
    } else {
        Ok(format!("{mount_prefix}/{drive}/{relative}"))
    }
}

/// Convert a windows path to a git-bash style posix path string
/// (e.g. `C:\foo` -> `/c/foo`).
///
/// # Errors
/// Errors if `p` is not an absolute local-disk windows path.
#[cfg(windows)]
pub fn windows_path_to_windows_bash<P: AsRef<Path>>(p: P) -> Result<String> {
    windows_path_to_posix(p.as_ref(), "")
}

/// Convert a windows path to a wsl path string (e.g. `C:\foo` -> `/mnt/c/foo`).
///
/// # Errors
/// Errors if `p` is not an absolute local-disk windows path.
#[cfg(windows)]
pub fn windows_path_to_wsl<P: AsRef<Path>>(p: P) -> Result<String> {
    windows_path_to_posix(p.as_ref(), "/mnt")
}

pub trait FileNameExt {
    /// Preserve the full extension including compound ones like `.tar.gz`.
    fn preserve_extension(&self) -> &str;
}

impl FileNameExt for str {
    fn preserve_extension(&self) -> &str {
        let lower = self.to_ascii_lowercase();
        // Long tar compounds must be kept whole (`.tar.gz`, not `.gz`). Short
        // aliases (`.tgz`) yield the same result as the plain fallback below;
        // sharing the table keeps the two lists from drifting apart.
        if let Some((ext, _)) = TAR_EXTENSIONS.iter().find(|(ext, _)| lower.ends_with(ext)) {
            // ASCII-only lowercasing above preserves byte length.
            return &self[self.len() - ext.len()..];
        }
        if let Some(dot_pos) = self.rfind('.') {
            return &self[dot_pos..];
        }
        ""
    }
}

impl FileNameExt for OsStr {
    fn preserve_extension(&self) -> &str {
        self.to_str().map_or("", FileNameExt::preserve_extension)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_name() {
        let tempdir = tempfile::tempdir().unwrap();
        let base = tempdir.path();

        let file1 = base.join("target.txt");
        let dir1 = base.join("nested");
        let file2 = dir1.join("target.txt");
        let file3 = dir1.join("ignore.txt");

        fs::create_dir_all(&dir1).unwrap();
        fs::write(&file1, "1").unwrap();
        fs::write(&file2, "2").unwrap();
        fs::write(&file3, "3").unwrap();

        let mut results = base.glob_name("target.txt");
        results.sort(); // sorted for stable assertions

        assert_eq!(results.len(), 2);
        assert!(results.contains(&file1));
        assert!(results.contains(&file2));

        // No match.
        assert_eq!(base.glob_name("non_existent.txt"), [] as [PathBuf; 0]);
    }

    #[test]
    fn test_create_dir_if_not_exist() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("new_dir");

        // First creation.
        path.create_dir_if_not_exist().unwrap();
        assert!(path.exists() && path.is_dir());

        // Idempotent: repeated calls must not error.
        path.create_dir_if_not_exist().unwrap();

        // A same-named FILE on the path must error (it is not a dir).
        let file_path = tempdir.path().join("conflict_file");
        fs::write(&file_path, "test").unwrap();
        assert!(file_path.create_dir_if_not_exist().is_err());
    }

    #[test]
    fn test_remove_all_allow_missing() {
        let tempdir = tempfile::tempdir().unwrap();
        let base = tempdir.path();

        // 1. Remove a directory.
        let dir_path = base.join("dir_to_remove");
        fs::create_dir_all(&dir_path).unwrap();
        dir_path.remove_all_allow_missing().unwrap();
        assert!(!dir_path.exists());

        // 2. Remove a file.
        let file_path = base.join("file_to_remove.txt");
        fs::write(&file_path, "test").unwrap();
        file_path.remove_all_allow_missing().unwrap();
        assert!(!file_path.exists());

        // 3. Remove a missing path (must not error).
        let missing_path = base.join("not_exist");
        assert!(missing_path.remove_all_allow_missing().is_ok());

        // 4. Remove twice (must not error).
        assert!(dir_path.remove_all_allow_missing().is_ok());
    }

    #[test]
    fn test_preserve_extension() {
        assert_eq!("foo.tar.gz".preserve_extension(), ".tar.gz");
        assert_eq!(
            "foo-v1.0-linux-amd64.tar.gz".preserve_extension(),
            ".tar.gz"
        );
        assert_eq!("foo.tar.bz2".preserve_extension(), ".tar.bz2");
        assert_eq!("foo.tar.xz".preserve_extension(), ".tar.xz");
        assert_eq!("foo.tar.zst".preserve_extension(), ".tar.zst");
        assert_eq!("foo.tar.zstd".preserve_extension(), ".tar.zstd");
        assert_eq!("foo.tgz".preserve_extension(), ".tgz");
        assert_eq!("foo.zip".preserve_extension(), ".zip");
        assert_eq!("foo.7z".preserve_extension(), ".7z");
        assert_eq!("foo.exe".preserve_extension(), ".exe");
        assert_eq!("foo".preserve_extension(), "");
        assert_eq!("".preserve_extension(), "");
        assert_eq!("foo.TAR.GZ".preserve_extension(), ".TAR.GZ");
        assert_eq!("FOO.tar.gz".preserve_extension(), ".tar.gz");
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_paths_conversion() {
        // Regular paths.
        let path1 = "C:\\Users\\lxl\\bpm\\bin";
        assert_eq!(
            windows_path_to_windows_bash(path1).unwrap(),
            "/c/Users/lxl/bpm/bin"
        );
        assert_eq!(
            windows_path_to_wsl(path1).unwrap(),
            "/mnt/c/Users/lxl/bpm/bin"
        );

        // Mixed separators.
        let path2 = "D:/projects\\test";
        assert_eq!(
            windows_path_to_windows_bash(path2).unwrap(),
            "/d/projects/test"
        );
        assert_eq!(windows_path_to_wsl(path2).unwrap(), "/mnt/d/projects/test");

        // Drive letter only.
        let path3 = "E:\\";
        assert_eq!(windows_path_to_windows_bash(path3).unwrap(), "/e");
        assert_eq!(windows_path_to_wsl(path3).unwrap(), "/mnt/e");

        // UNC/network (a domain home dir can be one) and relative paths have
        // no drive letter: an understandable error instead of the old panic.
        for bad in [
            r"\\server\share\bin",
            r"\\?\UNC\server\share",
            "relative\\path",
        ] {
            assert!(
                windows_path_to_windows_bash(bad)
                    .unwrap_err()
                    .to_string()
                    .contains("UNC/network"),
                "{bad}"
            );
            assert!(windows_path_to_wsl(bad).is_err(), "{bad}");
        }
    }
}
