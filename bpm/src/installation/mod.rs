mod download;
mod unzip;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use log::{info, warn};

use crate::{
    cli::DRY_RUN,
    config::Config,
    storage::{db::DbOperation, Repo},
    utils::path::PathExt,
};

/// check if the given path contains only one file.
#[inline]
pub fn only_one_file_in_dir(path: impl AsRef<Path>) -> std::io::Result<Option<PathBuf>> {
    let path = path.as_ref();
    let items = std::fs::read_dir(path)?.collect::<Vec<_>>();
    if items.len() == 1 {
        Ok(Some(
            path.join(items.into_iter().next().unwrap()?.file_name()),
        ))
    } else {
        Ok(None)
    }
}

/// move files from one dir to another dir recursively. the function `f` is
/// called for each file after moving it.
///
/// the files will be moved before their parent directory.
fn move_files_recursively<F>(src_dir: &Path, dst_dir: &Path, f: &mut F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    dst_dir.create_dir_if_not_exist()?;
    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst_dir.join(file_name);
        if src_path.is_dir() {
            dst_path.create_dir_if_not_exist()?;
            move_files_recursively(&src_path, &dst_path, f)?;
            if !*DRY_RUN.read().unwrap() {
                fs::remove_dir(&src_path)?;
            }
            info!("Moving dir : {:?} -> {:?}", src_path, dst_path);
            f(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            if !*DRY_RUN.read().unwrap() {
                fs::rename(&src_path, &dst_path)?;
            }
            info!("Moving file: {:?} -> {:?}", src_path, dst_path);
            f(&src_path, &dst_path)?;
        } else {
            warn!("Skipping non-file & non-directory: {:?}", src_path);
        }
    }
    Ok(())
}

/// check if the folder has only one `.msi` file and install it.
///
/// # Returns
///
/// `true` if installed successfully, `false` if not.
#[cfg(windows)]
fn check_and_install_msi(src: impl AsRef<Path>) -> std::io::Result<bool> {
    let src = src.as_ref();
    if let Some(file) = only_one_file_in_dir(src)? {
        if file.extension() == Some(std::ffi::OsStr::new("msi")) {
            info!("Start to install msi file {}.", file.display());
            if *DRY_RUN.read().unwrap() {
                info!("Dry run, skip installation.");
                return Ok(true);
            }
            let mut command = std::process::Command::new("msiexec.exe");
            command.arg("/i");
            command.arg(file);
            command.arg("/quiet");
            command.arg("/qr");
            command.arg("/norestart");
            command.spawn()?.wait()?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Install a file to a dir, with the given mode, keep the file name unchanged.
#[cfg(unix)]
fn install_to_dir_with_mode(src: impl AsRef<Path>, dst: impl AsRef<Path>, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::copy(
        &src,
        &dst.as_ref()
            .join(src.as_ref().file_name().expect("No file name")),
    )?;
    let permissions = fs::Permissions::from_mode(mode);
    fs::set_permissions(&dst, permissions)?;
    Ok(())
}

#[cfg(unix)]
pub mod unixpath {
    use std::path::PathBuf;

    use home::home_dir;

    static IS_ROOT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

    #[inline]
    fn is_root() -> bool {
        *IS_ROOT.get_or_init(|| unsafe { libc::getuid() } == 0)
    }

    #[inline]
    pub fn root() -> PathBuf {
        if is_root() {
            PathBuf::from("/usr")
        } else {
            home_dir()
                .expect("Failed to get home directory.")
                .join(".local")
        }
    }

    #[inline]
    pub fn bin() -> PathBuf {
        root().join("bin")
    }

    #[inline]
    pub fn lib() -> PathBuf {
        root().join("lib")
    }

    #[inline]
    pub fn share() -> PathBuf {
        root().join("share")
    }

    #[inline]
    pub fn include() -> PathBuf {
        root().join("include")
    }

    #[inline]
    pub fn services() -> PathBuf {
        if is_root() {
            PathBuf::from("/etc/systemd/system")
        } else {
            home_dir()
                .expect("Failed to get home directory.")
                .join(".config")
                .join("systemd")
                .join("user")
        }
    }
}

pub trait Installation {
    fn install(&mut self, src: impl AsRef<Path>, config: &Config<'_>) -> Result<()>;
    fn uninstall(&mut self, config: &Config<'_>) -> Result<()>;
    #[cfg(windows)]
    fn create_binary_link(&mut self, src: impl AsRef<Path>, config: &Config<'_>) -> Result<()>;
}

#[cfg(windows)]
impl Installation for Repo {
    /// Install files to a windows system.
    ///
    /// 1. try to install msi file.
    /// 2. move all files to the folder.
    /// 3. make lnk (for GUI binary files).
    /// 4. make a cmd for CLI binary files.
    /// 5. make a bash script for using in windows bash and WSL.
    ///
    /// `path`: The "main path" dir of files to be installed.
    fn install(&mut self, src: impl AsRef<Path>, config: &Config<'_>) -> Result<()> {
        if check_and_install_msi(src.as_ref())? {
            if !*DRY_RUN.read().unwrap() {
                config.db().insert_repo(self.clone())?;
            }
            return Ok(());
        }

        let app_dir = config.app_path().join(&self.name);
        let bin_dir = config.bin_path().join(&self.name);
        app_dir.create_dir_if_not_exist()?;
        bin_dir.create_dir_if_not_exist()?;

        move_files_recursively(src.as_ref(), app_dir.as_path(), &mut |_src, dst| {
            self.installed_files.push(dst.to_path_buf());
            Ok(())
        })?;

        self.create_binary_link(src, config)?;
        config.db().insert_repo(self.clone())?;
        Ok(())
    }

    fn uninstall(&mut self, config: &Config<'_>) -> Result<()> {
        if *DRY_RUN.read().unwrap() {
            for file in &self.installed_files {
                info!("dry run: Remove file: `{:?}`", file);
            }
            return Ok(());
        }
        let install_position = &config.install_position;
        for file in &self.installed_files {
            assert!(
                file.is_subpath_of(install_position),
                "UNSAFE REMOVE! trying to remove: {}",
                file.display()
            );
            file.remove_all_allow_missing()?;
        }
        config.db().remove_repo(&self.name)?;
        Ok(())
    }

    /// Create a link for binary files, so that we can call it in cmd, windows
    /// bash and WSL.
    fn create_binary_link(&mut self, src: impl AsRef<Path>, config: &Config<'_>) -> Result<()> {
        use mslnk::ShellLink;
        use path_absolutize::Absolutize;

        use crate::utils::path::{windows_path_to_windows_bash, windows_path_to_wsl};

        let bin_paths = src.as_ref().glob_name(&self.bin_name);
        if bin_paths.is_empty() {
            warn!("No binary file found in {:?}", src.as_ref());
            return Ok(());
        }
        // for user hinting
        let temp: String = bin_paths
            .iter()
            .map(|x| {
                format!(
                    "`{}`",
                    x.with_extension("").file_name().unwrap().to_str().unwrap()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        for bin_path in bin_paths {
            let temp = bin_path.with_extension("");
            let bin_name = temp.file_name().context("Failed to get file name.")?;
            let lnk_path = config.bin_path().join(bin_name).with_extension("lnk");
            let cmd_path = config.bin_path().join(bin_name).with_extension("cmd");
            let sh_path = config.bin_path().join(bin_name);
            if *DRY_RUN.read().unwrap() {
                info!("dry run: Create lnk: {:?} -> {:?}", bin_path, lnk_path);
                info!("dry run: Create cmd: {:?} -> {:?}", bin_path, cmd_path);
                info!("dry run: Create sh: {:?} -> {:?}", bin_path, cmd_path);
                continue;
            }
            // 删除现有的文件
            if lnk_path.exists() {
                fs::remove_file(&lnk_path).expect("Failed to remove lnk");
            }
            if cmd_path.exists() {
                fs::remove_file(&cmd_path).expect("Failed to remove cmd");
            }
            if sh_path.exists() {
                fs::remove_file(&sh_path).expect("Failed to remove sh");
            }

            // 创建 lnk
            let sl = ShellLink::new(&bin_path)?;
            sl.create_lnk(&lnk_path)?;
            info!("Create lnk: {:?} -> {:?}", bin_path, lnk_path);
            self.installed_files.push(lnk_path);

            // 创建 cmd
            let cmd_content = format!(
                r#"@echo off
"{}" %*"#,
                bin_path
                    .absolutize()
                    .expect("absolutize should be ok")
                    .display()
            );
            fs::write(&cmd_path, cmd_content)?;
            info!("Create cmd: {:?} -> {:?}", bin_path, cmd_path);
            self.installed_files.push(cmd_path);

            // 创建 sh
            let sh_content = format!(
                r#"#!/bin/sh
if [ "$(uname)" != "Linux" ]; then
    "{}" $*
else
    "{}" $*
fi"#,
                windows_path_to_windows_bash(&bin_path),
                windows_path_to_wsl(&bin_path)
            );
            std::fs::write(&sh_path, sh_content)?;
            info!("Create sh: {:?} -> {:?}", bin_path, sh_path);
            self.installed_files.push(sh_path);
        }
        info!("Successfully installed `{}`.", self.name);
        info!(
            "You can press `Win+r`, enter {} to start software, or execute in cmd.",
            temp
        );
        Ok(())
    }
}

#[cfg(unix)]
impl Installation for Repo {
    fn install(&mut self, src: impl AsRef<Path>, config: &Config<'_>) -> Result<()> {
        todo!()
    }
    fn uninstall(&mut self, config: &Config<'_>) -> Result<()> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::Write,
    };

    use tempfile::tempdir;

    use super::*;
    use crate::utils::log_init;

    #[test]
    fn test_only_one_file_in_dir() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path();
        let file1_path = path.join("file1.txt");
        let mut file1 = File::create(&file1_path)?;
        writeln!(file1, "This is file 1")?;
        assert!(only_one_file_in_dir(path)? == Some(file1_path));

        assert!((only_one_file_in_dir("test_assets")?).is_none());
        Ok(())
    }

    #[test]
    fn test_move_files_recursively() -> Result<()> {
        log_init();

        // 创建一个临时目录作为源目录
        let src_dir = tempdir()?;
        let src_path = src_dir.path();

        // 在源目录中创建文件和文件夹
        let file1_path = src_path.join("file1.txt");
        File::create(&file1_path)?;

        let file2_path = src_path.join("file2.txt");
        File::create(&file2_path)?;

        let subdir_path = src_path.join("subdir");
        fs::create_dir(&subdir_path)?;

        let subfile1_path = subdir_path.join("subfile1.txt");
        File::create(&subfile1_path)?;

        // 创建一个临时目录作为目标目录
        let dst_dir = tempdir()?;
        let dst_path = dst_dir.path();

        let mut record = vec![];

        // 执行递归文件移动
        move_files_recursively(src_path, dst_path, &mut |_, dst| {
            record.push(dst.to_path_buf());
            Ok(())
        })?;

        // 检查文件是否被成功移动
        assert!(dst_path.join("file1.txt").exists());
        assert!(dst_path.join("file2.txt").exists());
        assert!(dst_path.join("subdir").exists());
        assert!(dst_path.join("subdir/subfile1.txt").exists());
        // 检查 record 是否包含正确的文件路径，以及次序是否正确
        assert!(record.len() == 4);
        assert!(
            record
                .iter()
                .position(|x| x == &dst_path.join("subdir/subfile1.txt"))
                .unwrap()
                < record
                    .iter()
                    .position(|x| x == &dst_path.join("subdir"))
                    .unwrap()
        );

        // 检查源目录内容是否已经删除
        assert!(!file1_path.exists());
        assert!(!file2_path.exists());
        assert!(!subfile1_path.exists());
        assert!(!subdir_path.exists());
        Ok(())
    }
}
