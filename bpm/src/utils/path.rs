use std::{
    fs,
    path::{Path, PathBuf},
};

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
    /// check if `self` is subpath of `other`
    fn is_subpath_of(&self, other: impl AsRef<Path>) -> bool;
}

impl<P: AsRef<Path>> PathExt for P {
    /// Find all files with the given name in the given directory recursively.
    ///
    /// The pattern should be only filename, and should not contains `*` or `?` or other wildcard characters.
    fn glob_name(&self, pattern: &str) -> Vec<PathBuf> {
        let mut results = Vec::new();
        find_files_by_name_inner(self.as_ref(), pattern, &mut results);
        results
    }
    fn create_dir_if_not_exist(&self) -> Result<(), std::io::Error> {
        match fs::create_dir_all(self) {
            x @ Ok(()) => x,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn remove_all_allow_missing(&self) -> Result<(), std::io::Error> {
        match fs::remove_dir_all(self) {
            x @ Ok(()) => x,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn is_subpath_of(&self, other: impl AsRef<Path>) -> bool {
        let mut pat = self.as_ref();
        if pat == other.as_ref() {
            return true;
        }
        while let Some(p) = pat.parent() {
            if p == other.as_ref() {
                return true;
            }
            pat = p;
        }
        false
    }
}

/// Find all files with the given name in the given directory recursively.
///
/// # Arguments
///
/// - `dir`: the directory to search in
/// - `file_name`: the name of the file to search for
/// - `results`: the vector to add the found files to
///
/// # Errors
///
/// - Propagates I/O errors from `fs::read_dir`
fn find_files_by_name_inner(dir: &Path, file_name: &str, results: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                find_files_by_name_inner(&path, file_name, results);
            } else if let Some(name) = path.file_name() {
                if name == file_name {
                    results.push(path);
                }
            }
        }
    }
}

#[cfg(windows)]
/// convert a windows path to posix path string.
///
/// # Examples
///
/// ```
/// use bpm::utils::path::windows_path_to_windows_bash;
/// #[cfg(windows)]
/// assert_eq!(windows_path_to_windows_bash("C:\\Users\\lxl\\bpm\\bin"), "/c/Users/lxl/bpm/bin");
/// ```
///
/// # Panics
///
/// Panics if `p` is not a valid windows path.
pub fn windows_path_to_windows_bash<P: AsRef<Path>>(p: P) -> String {
    let p = PathBuf::from(p.as_ref());

    let drive = p
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_str()
        .unwrap()
        .trim_end_matches(':')
        .to_lowercase();
    let relative_path = p
        .strip_prefix(p.ancestors().last().unwrap())
        .unwrap()
        .to_str()
        .unwrap();

    format!("/{}/{}", drive, relative_path.replace('\\', "/"))
}

#[cfg(windows)]
/// convert a windows path to wsl path string.
///
/// # Examples
///
/// ```
/// use bpm::utils::path::windows_path_to_wsl;
/// #[cfg(windows)]
/// assert_eq!(windows_path_to_wsl("C:\\Users\\lxl\\bpm\\bin"), "/mnt/c/Users/lxl/bpm/bin");
/// ```
///
/// # Panics
///
/// Panics if `p` is not a valid windows path.
pub fn windows_path_to_wsl<P: AsRef<Path>>(p: P) -> String {
    let windows_bash_path = windows_path_to_windows_bash(p);
    format!("/mnt/{}", windows_bash_path.strip_prefix('/').unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_name() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test1.txt");
        let path2 = tempdir.path().join("test2").join("test3.txt");
        path2.parent().unwrap().create_dir_if_not_exist().unwrap();
        fs::write(&path, "test").unwrap();
        fs::write(&path2, "test").unwrap();
        assert_eq!(tempdir.path().glob_name("test1.txt").len(), 1);
        assert_eq!(tempdir.path().glob_name("test3.txt").len(), 1);
        assert_eq!(tempdir.path().glob_name("testxxxx").len(), 0);
    }

    #[test]
    fn test_create_dir_if_not_exist() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test_create_dir_if_not_exist");
        path.create_dir_if_not_exist().unwrap();
        assert!(path.exists());
        assert!(path.is_dir());
        path.create_dir_if_not_exist().unwrap();
        path.create_dir_if_not_exist().unwrap();
        path.create_dir_if_not_exist().unwrap();
    }

    #[test]
    fn test_remove_all_allow_missing() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test_remove_all_allow_missing");
        path.create_dir_if_not_exist().unwrap();
        assert!(path.exists());
        assert!(path.is_dir());
        path.remove_all_allow_missing().unwrap();
        path.remove_all_allow_missing().unwrap();
        path.remove_all_allow_missing().unwrap();
    }

    #[test]
    fn test_is_subpath_of() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test_is_subpath");
        let path2 = path.join("test_is_subpath2");
        assert!(path.is_subpath_of(&path));
        assert!(!path.is_subpath_of(&path2));
        assert!(path2.is_subpath_of(&path));
    }
}
