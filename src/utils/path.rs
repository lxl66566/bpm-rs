use std::{
    fs,
    path::{Component, Path, PathBuf, Prefix},
};

use walkdir::WalkDir;

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
        // 使用 walkdir 库，避免手写递归导致栈溢出，且过滤更优雅
        WalkDir::new(self)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_type().is_file()) // 只保留文件
            .filter(|e| e.file_name() == pattern) // 匹配文件名
            .map(|e| e.path().to_path_buf())
            .collect()
    }

    fn create_dir_if_not_exist(&self) -> Result<(), std::io::Error> {
        // fs::create_dir_all 本身如果遇到目录已存在，就不会报错，无需手动 match 拦截
        fs::create_dir_all(self)
    }

    fn remove_all_allow_missing(&self) -> Result<(), std::io::Error> {
        let path = self.as_ref();

        // 优先判断是否是目录（如果不存在会返回 false）
        if path.is_dir() {
            match fs::remove_dir_all(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            // 是文件或者路径压根不存在
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        }
    }
}

#[cfg(windows)]
fn extract_windows_drive_and_relative(path: &Path) -> (char, String) {
    let mut components = path.components();

    // 提取盘符
    let drive = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(d) | Prefix::VerbatimDisk(d) => (d as char).to_ascii_lowercase(),
            _ => panic!("Path must start with a disk drive (e.g., C:)"),
        },
        _ => panic!("Path must be an absolute Windows path"),
    };

    // 收集剩余路径（自动处理掉 RootDir 即 "C:\" 中的 "\"）
    let relative: Vec<String> = components
        .filter(|c| !matches!(c, Component::RootDir))
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    (drive, relative.join("/"))
}

/// convert a windows path to posix path string.
///
/// # Panics
/// Panics if `p` is not a valid absolute windows path.
#[cfg(windows)]
pub fn windows_path_to_windows_bash<P: AsRef<Path>>(p: P) -> String {
    let (drive, relative) = extract_windows_drive_and_relative(p.as_ref());
    if relative.is_empty() {
        format!("/{drive}")
    } else {
        format!("/{drive}/{relative}")
    }
}

/// convert a windows path to wsl path string.
///
/// # Panics
/// Panics if `p` is not a valid absolute windows path.
#[cfg(windows)]
pub fn windows_path_to_wsl<P: AsRef<Path>>(p: P) -> String {
    let (drive, relative) = extract_windows_drive_and_relative(p.as_ref());
    if relative.is_empty() {
        format!("/mnt/{drive}")
    } else {
        format!("/mnt/{drive}/{relative}")
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
        results.sort(); // 排序以确保断言稳定

        assert_eq!(results.len(), 2);
        assert!(results.contains(&file1));
        assert!(results.contains(&file2));

        // 测试找不到的情况
        assert!(base.glob_name("non_existent.txt").is_empty());
    }

    #[test]
    fn test_create_dir_if_not_exist() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("new_dir");

        // 第一次创建
        path.create_dir_if_not_exist().unwrap();
        assert!(path.exists() && path.is_dir());

        // 幂等性测试（重复调用不报错）
        path.create_dir_if_not_exist().unwrap();

        // 测试路径上存在一个【同名文件】导致的错误
        let file_path = tempdir.path().join("conflict_file");
        fs::write(&file_path, "test").unwrap();
        let result = file_path.create_dir_if_not_exist();
        assert!(result.is_err()); // 应该返回 OS Error，因为那是文件不是目录
    }

    #[test]
    fn test_remove_all_allow_missing() {
        let tempdir = tempfile::tempdir().unwrap();
        let base = tempdir.path();

        // 1. 测试删除目录
        let dir_path = base.join("dir_to_remove");
        fs::create_dir_all(&dir_path).unwrap();
        dir_path.remove_all_allow_missing().unwrap();
        assert!(!dir_path.exists());

        // 2. 测试删除文件
        let file_path = base.join("file_to_remove.txt");
        fs::write(&file_path, "test").unwrap();
        file_path.remove_all_allow_missing().unwrap();
        assert!(!file_path.exists());

        // 3. 测试删除不存在的路径（不应报错）
        let missing_path = base.join("not_exist");
        assert!(missing_path.remove_all_allow_missing().is_ok());

        // 4. 测试重复删除（不应报错）
        assert!(dir_path.remove_all_allow_missing().is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_paths_conversion() {
        // 测试常规路径
        let path1 = "C:\\Users\\lxl\\bpm\\bin";
        assert_eq!(windows_path_to_windows_bash(path1), "/c/Users/lxl/bpm/bin");
        assert_eq!(windows_path_to_wsl(path1), "/mnt/c/Users/lxl/bpm/bin");

        // 测试包含斜杠的混合路径
        let path2 = "D:/projects\\test";
        assert_eq!(windows_path_to_windows_bash(path2), "/d/projects/test");
        assert_eq!(windows_path_to_wsl(path2), "/mnt/d/projects/test");

        // 测试只有盘符的情况
        let path3 = "E:\\";
        assert_eq!(windows_path_to_windows_bash(path3), "/e");
        assert_eq!(windows_path_to_wsl(path3), "/mnt/e");
    }
}
