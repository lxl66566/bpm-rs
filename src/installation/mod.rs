pub mod download;
pub mod unzip;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use log::info;

use crate::{context::Context, utils::path::PathExt};

#[inline]
pub fn only_one_file_in_dir(path: impl AsRef<Path>) -> std::io::Result<Option<PathBuf>> {
    let mut iter = fs::read_dir(path)?;
    match (iter.next(), iter.next()) {
        (Some(Ok(entry)), None) => Ok(Some(entry.path())),
        _ => Ok(None),
    }
}

// 文件移动直接使用 fs_extra 库，支持跨磁盘 (Cross-Device) 移动。
fn move_dir_content(src_dir: &Path, dst_dir: &Path, dry_run: bool) -> Result<()> {
    if dry_run {
        info!(
            "Dry run: moving contents from `{:?}` to `{:?}`",
            src_dir.display(),
            dst_dir.display()
        );
        return Ok(());
    }

    dst_dir.create_dir_if_not_exist()?;
    let mut options = fs_extra::dir::CopyOptions::new();
    options.content_only = true; // 只移动内容，不包含外层文件夹本身
    options.overwrite = true;

    // move_dir 会自动处理同磁盘 rename 和跨磁盘 copy+delete
    fs_extra::dir::move_dir(src_dir, dst_dir, &options)?;
    info!(
        "Moved contents: `{:?}` -> `{:?}`",
        src_dir.display(),
        dst_dir.display()
    );
    Ok(())
}

#[cfg(unix)]
fn rename_old(path: &Path) -> std::io::Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let new_ext = format!("{ext}.old");
    fs::rename(path, path.with_extension(new_ext))
}

#[cfg(unix)]
fn restore_old(path: &Path) -> std::io::Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let old_ext = format!("{ext}.old");
    let old = path.with_extension(old_ext);
    if old.exists() {
        fs::rename(&old, path)?;
        info!("Restoring {old:?} -> {path:?}");
    }
    Ok(())
}

#[cfg(windows)]
fn check_and_install_msi(src: impl AsRef<Path>, dry_run: bool) -> Result<bool> {
    let src = src.as_ref();
    if let Some(file) = only_one_file_in_dir(src)?
        && file
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("msi"))
    {
        info!("Start to install msi file {}.", file.display());
        if dry_run {
            info!("Dry run, skip installation.");
            return Ok(true);
        }
        std::process::Command::new("msiexec.exe")
            .args(["/i", file.to_str().unwrap(), "/quiet", "/qr", "/norestart"])
            .status()?;
        return Ok(true);
    }
    Ok(false)
}

pub trait Installation {
    fn install(&mut self, src: impl AsRef<Path>, ctx: &Context) -> Result<()>;
    fn uninstall(&mut self, ctx: &Context) -> Result<()>;
}

#[cfg(unix)]
mod unix_impl {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    use anyhow::{Result, bail};
    use log::{debug, info, warn};
    use walkdir::WalkDir;

    use super::{only_one_file_in_dir, rename_old};
    use crate::{
        context::Context, installation::Installation, storage::Repo, utils::path::PathExt,
    };

    pub struct UnixPaths {
        root: PathBuf,
    }

    impl UnixPaths {
        pub fn new() -> Self {
            let root = if crate::utils::is_root() {
                PathBuf::from("/usr")
            } else {
                home::home_dir()
                    .expect("Failed to get home directory")
                    .join(".local")
            };
            Self { root }
        }
        #[inline]
        pub fn bin(&self) -> PathBuf {
            self.root.join("bin")
        }
        #[inline]
        pub fn lib(&self) -> PathBuf {
            self.root.join("lib")
        }
        #[inline]
        pub fn share(&self) -> PathBuf {
            self.root.join("share")
        }
        #[inline]
        pub fn include(&self) -> PathBuf {
            self.root.join("include")
        }
        #[inline]
        pub fn services(&self) -> PathBuf {
            if crate::utils::is_root() {
                PathBuf::from("/etc/systemd/system")
            } else {
                home::home_dir().unwrap().join(".config/systemd/user")
            }
        }
    }

    fn install_file(
        src: &Path,
        dst: &Path,
        dry_run: bool,
        mode: Option<u32>,
        recorder: &mut Vec<PathBuf>,
    ) -> Result<()> {
        recorder.push(dst.to_path_buf());
        if dry_run {
            info!("dry run: {dst:?}");
            return Ok(());
        }

        if src.is_dir() {
            fs::create_dir_all(dst)?;
            info!("mkdir {src:?} -> {dst:?}");
            return Ok(());
        }

        if dst.exists() {
            rename_old(dst)?;
        }
        fs::copy(src, dst)?;
        info!("{src:?} -> {dst:?}");

        if let Some(m) = mode {
            fs::set_permissions(dst, fs::Permissions::from_mode(m))?;
        }
        Ok(())
    }

    fn merge_dir(from: &Path, to: &Path, dry_run: bool, recorder: &mut Vec<PathBuf>) -> Result<()> {
        for entry in WalkDir::new(from).min_depth(1) {
            let entry = entry?;
            let src = entry.path();
            let rel_path = src.strip_prefix(from)?;
            let dst = to.join(rel_path);

            if src.is_dir() {
                if !dry_run {
                    fs::create_dir_all(&dst)?;
                }
                recorder.push(dst);
            } else {
                install_file(src, &dst, dry_run, None, recorder)?;
            }
        }
        Ok(())
    }

    fn install_completions(
        path: &Path,
        paths: &UnixPaths,
        dry_run: bool,
        recorder: &mut Vec<PathBuf>,
    ) -> Result<()> {
        if !path.is_dir() {
            return Ok(());
        }
        debug!("installing completions from {path:?}");

        for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
            let src = entry.path();
            if !src.is_file() {
                continue;
            }

            let file_name = src.file_name().unwrap();
            let name_str = file_name.to_string_lossy();
            let mut dst = None;

            if name_str.ends_with(".fish") {
                dst = Some(
                    paths
                        .share()
                        .join("fish/vendor_completions.d")
                        .join(file_name),
                );
            } else if name_str.ends_with(".bash") {
                dst = Some(
                    paths
                        .share()
                        .join("bash-completion/completions")
                        .join(file_name),
                );
            } else if name_str.starts_with('_') {
                if fs::read_to_string(src).map_or(false, |c| c.contains("zsh")) {
                    dst = Some(paths.share().join("zsh/site-functions").join(file_name));
                }
            }

            if let Some(d) = dst {
                install_file(src, &d, dry_run, Some(0o644), recorder)?;
            }
        }
        Ok(())
    }

    impl Installation for Repo {
        fn install(&mut self, src: impl AsRef<Path>, ctx: &Context) -> Result<()> {
            let src = src.as_ref();
            let unix_paths = UnixPaths::new();
            let dry_run = ctx.dry_run;
            let mut first_layer: Vec<_> = std::fs::read_dir(src)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .collect();

            if first_layer.is_empty() {
                bail!("{} is empty, nothing to install", src.display());
            }

            if self.one_bin || first_layer.len() == 1 {
                let bin_file = if first_layer.len() == 1 && first_layer[0].is_file() {
                    first_layer[0].clone()
                } else {
                    let candidates = src.as_ref().glob_name(&self.bin_name);
                    match candidates.into_iter().next() {
                        Some(f) => f,
                        None => {
                            warn!("No binary file found for {}", self.bin_name);
                            return Ok(());
                        }
                    }
                };

                if bin_file.is_file() {
                    debug!("selected binary: {bin_file:?}");
                    let dst = unix_paths.bin().join(bin_file.file_name().unwrap());
                    install_file(
                        &bin_file,
                        &dst,
                        dry_run,
                        Some(0o755),
                        &mut self.installed_files,
                    )?;
                    if self.one_bin {
                        return Ok(());
                    }
                }
            }

            for file in &first_layer {
                let name = file.file_name().unwrap_or_default().to_string_lossy();
                match name.as_ref() {
                    "usr" => merge_dir(
                        file,
                        &PathBuf::from("/usr"),
                        dry_run,
                        &mut self.installed_files,
                    )?,
                    "lib" => {
                        merge_dir(file, &unix_paths.lib(), dry_run, &mut self.installed_files)?
                    }
                    "include" => merge_dir(
                        file,
                        &unix_paths.include(),
                        dry_run,
                        &mut self.installed_files,
                    )?,
                    "share" => merge_dir(
                        file,
                        &unix_paths.share(),
                        dry_run,
                        &mut self.installed_files,
                    )?,
                    "bin" => {
                        merge_dir(file, &unix_paths.bin(), dry_run, &mut self.installed_files)?
                    }
                    "man" => merge_dir(
                        file,
                        &unix_paths.share().join("man"),
                        dry_run,
                        &mut self.installed_files,
                    )?,
                    n if n.starts_with("complet") => {
                        install_completions(file, &unix_paths, dry_run, &mut self.installed_files)?;
                    }
                    n if n == self.bin_name && file.is_file() => {
                        let dst = unix_paths.bin().join(file.file_name().unwrap());
                        install_file(file, &dst, dry_run, Some(0o755), &mut self.installed_files)?;
                    }
                    _ => {
                        debug!("cannot match {name}.");
                    }
                }
            }

            for service_file in walk_files(src, ".service") {
                let dst = unix_paths
                    .services()
                    .join(service_file.file_name().unwrap());
                install_file(
                    &service_file,
                    &dst,
                    dry_run,
                    Some(0o644),
                    &mut self.installed_files,
                )?;
            }

            let has_bin = self
                .installed_files
                .iter()
                .any(|f| f.to_str().map(|s| s.contains("bin")).unwrap_or(false));
            if !has_bin {
                warn!("No binary file found, please check the release package.");
            }

            Ok(())
        }
        fn uninstall(&mut self, ctx: &Context) -> Result<()> {
            if ctx.dry_run {
                for file in &self.installed_files {
                    info!("dry run: Remove file: {file:?}");
                }
                return Ok(());
            }

            for file in self.installed_files.iter().rev() {
                if file.is_dir() {
                    let _ = fs::remove_dir(file);
                } else {
                    let _ = fs::remove_file(file);
                }
                info!("deleting {file:?}");
                super::restore_old(file)?;
            }

            Ok(())
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::{
        fs::{self, File},
        io::Cursor,
        path::{Path, PathBuf},
    };

    use anyhow::Result;
    use log::{debug, info, warn};
    use walkdir::WalkDir;

    use super::{check_and_install_msi, move_dir_content};
    use crate::{
        context::Context,
        installation::Installation,
        storage::{Repo, db::DbOperation},
        utils::path::PathExt,
    };

    impl Installation for Repo {
        fn install(&mut self, src: impl AsRef<Path>, ctx: &Context) -> Result<()> {
            let src = src.as_ref();

            if check_and_install_msi(src, ctx.dry_run)? {
                if !ctx.dry_run {
                    ctx.db()?.insert_repo(self.clone())?;
                }
                return Ok(());
            }

            let app_dir = ctx.app_path().join(&self.name);

            // 直接移动文件夹，不再记录每一个内部文件。
            move_dir_content(src, &app_dir, ctx.dry_run)?;
            self.installed_files.push(app_dir.clone());

            self.create_binary_links(ctx)?;
            Ok(())
        }

        fn uninstall(&mut self, ctx: &Context) -> Result<()> {
            if ctx.dry_run {
                for file in &self.installed_files {
                    info!("dry run: Remove file: {:?}", file.display());
                }
                return Ok(());
            }
            let app_path = ctx.app_path();
            let install_parent = app_path.parent().unwrap_or(Path::new("."));
            for file in &self.installed_files {
                if !file.starts_with(install_parent) {
                    anyhow::bail!("UNSAFE REMOVE! trying to remove: {}", file.display());
                }
                file.remove_all_allow_missing()?;
            }
            Ok(())
        }
    }

    trait WindowsBinaryLinks {
        fn create_binary_links(&mut self, ctx: &Context) -> Result<()>;
    }

    impl WindowsBinaryLinks for Repo {
        fn create_binary_links(&mut self, ctx: &Context) -> Result<()> {
            use path_absolutize::Absolutize;

            use crate::utils::path::{windows_path_to_windows_bash, windows_path_to_wsl};

            debug!("creating binary links for {self:?}");

            let app_dir = ctx.app_path().join(&self.name);
            let bin_path = ctx.bin_path();
            bin_path.create_dir_if_not_exist()?;

            let mut bin_files = app_dir.glob_name(&self.bin_name);
            if bin_files.is_empty() {
                // 兜底：扫描所有 .exe 文件
                bin_files = WalkDir::new(&app_dir)
                    .into_iter()
                    .filter_map(std::result::Result::ok)
                    .filter(|e| e.file_type().is_file())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                    })
                    .map(|e| e.path().to_path_buf())
                    .collect();
            }
            if bin_files.is_empty() {
                warn!(
                    "no binary file found in `{:?}`, skip creating links.",
                    app_dir.display()
                );
                return Ok(());
            }

            if !ctx.dry_run {
                let base_shim = ensure_base_shim(ctx)?;

                for bin_file in &bin_files {
                    let stem = bin_file.file_stem().unwrap().to_string_lossy().to_string();
                    let base = bin_path.join(&stem);

                    let exe_path = base.with_extension("exe");
                    let shim_cfg_path = base.with_extension("shim");
                    let sh_path = base.clone();

                    for p in [&exe_path, &shim_cfg_path, &sh_path] {
                        let _ = fs::remove_file(p);
                    }

                    fs::hard_link(&base_shim, &exe_path)?;
                    debug!("create exe shim: {:?}", exe_path.display());
                    self.installed_files.push(exe_path);

                    let target = bin_file.absolutize()?;
                    fs::write(&shim_cfg_path, format!("path = {}\n", target.display()))?;
                    debug!("create shim config: {:?}", shim_cfg_path.display());
                    self.installed_files.push(shim_cfg_path);

                    fs::write(
                        &sh_path,
                        format!(
                            "#!/bin/sh\nif [ \"$(uname)\" != \"Linux\" ]; then\n    \"{}\" \"$@\"\nelse\n    \"{}\" \"$@\"\nfi",
                            windows_path_to_windows_bash(bin_file),
                            windows_path_to_wsl(bin_file)
                        ),
                    )?;
                    debug!(
                        "create sh: `{:?}` -> `{:?}`",
                        bin_file.display(),
                        sh_path.display()
                    );
                    self.installed_files.push(sh_path);
                }
            }

            ensure_windows_path(&bin_path);
            Ok(())
        }
    }

    fn ensure_base_shim(ctx: &Context) -> Result<PathBuf> {
        let shim_path = ctx.shim_exe();
        if shim_path.exists() {
            return Ok(shim_path);
        }

        let compressed = include_bytes!(concat!(env!("OUT_DIR"), "/bpm-shim.exe.zst"));
        let mut decoder = zstd::Decoder::new(Cursor::new(compressed))?;
        let mut output = File::create(&shim_path)?;
        std::io::copy(&mut decoder, &mut output)?;
        info!("extracted base shim to {}", shim_path.display());
        Ok(shim_path)
    }

    fn ensure_windows_path(bin_path: &Path) {
        let bin_str = bin_path.to_string_lossy();
        if let Ok(path_str) = std::env::var("PATH")
            && path_str.to_lowercase().contains(&bin_str.to_lowercase())
        {
            return;
        }
        match windows_env::append("PATH", &bin_str) {
            Ok(()) => {
                info!("{bin_str} added to PATH. You may need to restart the terminal to apply.");
            }
            Err(e) => warn!("Failed to add {bin_str} to PATH: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    // =========================================================================
    // 跨平台通用功能测试
    // =========================================================================

    #[test]
    fn test_only_one_file_in_dir() {
        let dir = tempdir().unwrap();
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

    #[test]
    fn test_move_dir_content() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        // 构造源目录结构：
        // src/
        //  ├─ a.txt
        //  └─ sub/
        //      └─ b.txt
        fs::write(src_dir.path().join("a.txt"), "A").unwrap();
        fs::create_dir(src_dir.path().join("sub")).unwrap();
        fs::write(src_dir.path().join("sub/b.txt"), "B").unwrap();

        // 执行移动操作 (由于 dst_dir 也是临时目录，前提假定其自带的
        // create_dir_if_not_exist 不会报错) 注意：由于我们在主代码中引入了
        // utils::path::PathExt，此测试依赖该 trait 正常工作
        move_dir_content(src_dir.path(), dst_dir.path(), false).unwrap();

        // 验证文件是否已移动到目标路径
        assert!(dst_dir.path().join("a.txt").exists());
        assert!(dst_dir.path().join("sub/b.txt").exists());

        // 验证源路径里的内容是否已被清空
        assert!(!src_dir.path().join("a.txt").exists());
        assert!(!src_dir.path().join("sub").exists());
    }

    #[test]
    fn test_move_dir_content_dry_run() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        fs::write(src_dir.path().join("a.txt"), "A").unwrap();

        // dry_run = true
        move_dir_content(src_dir.path(), dst_dir.path(), true).unwrap();

        // 验证文件没有被移动
        assert!(src_dir.path().join("a.txt").exists());
        assert!(!dst_dir.path().join("a.txt").exists());
    }

    // =========================================================================
    // Unix 特有功能测试
    // =========================================================================
    #[cfg(unix)]
    mod unix_tests {
        use std::os::unix::fs::PermissionsExt;

        use super::*;
        use crate::utils::path::PathExt; // 确保作用域内存在需要的 Trait

        #[test]
        fn test_rename_and_restore_old() {
            let dir = tempdir().unwrap();
            let file_path = dir.path().join("binary.bin");
            let old_path = dir.path().join("binary.bin.old");

            // 1. 创建文件
            fs::write(&file_path, "executable code").unwrap();
            assert!(file_path.exists());
            assert!(!old_path.exists());

            // 2. 测试 rename_old
            super::super::rename_old(&file_path).unwrap();
            assert!(!file_path.exists(), "Original file should not exist");
            assert!(old_path.exists(), ".old file should exist");

            // 3. 测试 restore_old
            super::super::restore_old(&file_path).unwrap();
            assert!(file_path.exists(), "Original file should be restored");
            assert!(!old_path.exists(), ".old file should be removed");
        }

        #[test]
        fn test_unix_paths_generation() {
            let paths = super::super::unix_impl::UnixPaths::new();
            // 简单校验路径后缀是否符合预期（不强求前缀，因为它是动态的 based on root/home）
            assert!(paths.bin().ends_with("bin"));
            assert!(paths.lib().ends_with("lib"));
            assert!(paths.share().ends_with("share"));
            assert!(paths.include().ends_with("include"));
        }
    }

    // =========================================================================
    // Windows 特有功能测试
    // =========================================================================
    #[cfg(windows)]
    mod windows_tests {
        use super::*;

        #[test]
        fn test_check_and_install_msi_dry_run() {
            let dir = tempdir().unwrap();

            // 没有文件时，应该返回 false
            let res = super::super::check_and_install_msi(dir.path(), true).unwrap();
            assert!(!res);

            // 放一个 txt 文件，应该返回 false
            let txt_file = dir.path().join("test.txt");
            fs::write(&txt_file, "dummy").unwrap();
            let res = super::super::check_and_install_msi(dir.path(), true).unwrap();
            assert!(!res);

            // 替换成 msi 文件，应该匹配到并返回 true (由于是 dry_run 不会真实执行安装)
            fs::remove_file(&txt_file).unwrap();
            let msi_file = dir.path().join("installer.msi");
            fs::write(&msi_file, "dummy installer").unwrap();

            let res = super::super::check_and_install_msi(dir.path(), true).unwrap();
            assert!(res);
        }
    }
}
