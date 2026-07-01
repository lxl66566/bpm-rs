use std::{
    fs::{self, File},
    io::Cursor,
    path::{Path, PathBuf},
};

use anyhow::Result;
use log::{debug, info, warn};
use walkdir::WalkDir;

use super::{Installation, only_one_file_in_dir};
use crate::{
    context::Context,
    storage::{Repo, db::DbOperation},
    utils::path::PathExt,
};

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

/// Coupled filesystem + tracking helpers for an installed repo.
///
/// Each method touches **both** the filesystem and `installed_files` in one
/// call, so the two can never drift apart (which is how stale or missing
/// entries used to sneak in).
impl Repo {
    /// Remove pre-existing shim/link artifacts for the given base path.
    ///
    /// Besides the current shim files (`<base>.exe`, `<base>.shim` and the
    /// bare `<base>` sh script), this also cleans up legacy `<base>.lnk` /
    /// `<base>.cmd` files left behind by the old (python) bpm, so that an
    /// update fully replaces them with the new shim-based scripts.
    ///
    /// Removed paths are deleted from disk **and** untracked from
    /// `installed_files` together — missing files / untracked entries are
    /// silently ignored.
    fn remove_shim_artifacts(&mut self, base: &Path) {
        let paths: Vec<PathBuf> = std::iter::once(base.to_path_buf())
            .chain(
                ["exe", "shim", "lnk", "cmd"]
                    .into_iter()
                    .map(|e| base.with_extension(e)),
            )
            .collect();
        for p in &paths {
            let _ = fs::remove_file(p);
            self.installed_files.remove(p);
        }
    }

    /// Perform a filesystem operation and track `dst` as installed in a single
    /// step, so the on-disk state and `installed_files` can never drift apart.
    ///
    /// This is the add-side counterpart of [`Repo::remove_shim_artifacts`]: the
    /// operation runs first and `dst` is tracked only on success, which means a
    /// failed write never leaves a stale entry behind.
    fn add_installed_file(&mut self, dst: &Path, op: impl FnOnce() -> Result<()>) -> Result<()> {
        op()?;
        self.installed_files.insert(dst.to_path_buf());
        Ok(())
    }
}

impl Installation for Repo {
    fn install(&mut self, src: impl AsRef<Path>, ctx: &Context) -> Result<()> {
        let src = src.as_ref();

        if check_and_install_msi(src, ctx.dry_run)? {
            warn!(
                "MSI package '{}' was installed silently via msiexec. \
                 bpm cannot track or uninstall MSI-installed files — \
                 use Windows Settings > Apps to uninstall.",
                self.name
            );
            self.is_msi = true;
            if !ctx.dry_run {
                ctx.db()?.insert_repo(self.clone())?;
            }
            return Ok(());
        }

        let app_dir = ctx.app_path().join(&self.name);

        // 直接移动文件夹，不再记录每一个内部文件。
        self.add_installed_file(&app_dir, || move_dir_content(src, &app_dir, ctx.dry_run))?;

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

                // Remove any pre-existing artifacts before recreating, including
                // legacy `.lnk` / `.cmd` files left by the old (python) bpm so
                // that an update fully switches over to the new shims. This
                // also untracks them, keeping disk and installed_files in sync.
                self.remove_shim_artifacts(&base);

                self.add_installed_file(&exe_path, || {
                    fs::hard_link(&base_shim, &exe_path)?;
                    debug!("create exe shim: {:?}", exe_path.display());
                    Ok(())
                })?;

                let target = bin_file.absolutize()?;
                self.add_installed_file(&shim_cfg_path, || {
                    fs::write(&shim_cfg_path, format!("path = {}\n", target.display()))?;
                    debug!("create shim config: {:?}", shim_cfg_path.display());
                    Ok(())
                })?;

                self.add_installed_file(&sh_path, || {
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
                    Ok(())
                })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_dir_content() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();

        // 构造源目录结构：
        // src/
        //  ├─ a.txt
        //  └─ sub/
        //      └─ b.txt
        fs::write(src_dir.path().join("a.txt"), "A").unwrap();
        fs::create_dir(src_dir.path().join("sub")).unwrap();
        fs::write(src_dir.path().join("sub/b.txt"), "B").unwrap();

        move_dir_content(src_dir.path(), dst_dir.path(), false).unwrap();

        assert!(dst_dir.path().join("a.txt").exists());
        assert!(dst_dir.path().join("sub/b.txt").exists());
        assert!(!src_dir.path().join("a.txt").exists());
        assert!(!src_dir.path().join("sub").exists());
    }

    #[test]
    fn test_move_dir_content_dry_run() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();

        fs::write(src_dir.path().join("a.txt"), "A").unwrap();

        move_dir_content(src_dir.path(), dst_dir.path(), true).unwrap();

        assert!(src_dir.path().join("a.txt").exists());
        assert!(!dst_dir.path().join("a.txt").exists());
    }

    #[test]
    fn test_check_and_install_msi_dry_run() {
        let dir = tempfile::tempdir().unwrap();

        // 没有文件时，应该返回 false
        let res = check_and_install_msi(dir.path(), true).unwrap();
        assert!(!res);

        // 放一个 txt 文件，应该返回 false
        let txt_file = dir.path().join("test.txt");
        fs::write(&txt_file, "dummy").unwrap();
        let res = check_and_install_msi(dir.path(), true).unwrap();
        assert!(!res);

        // 替换成 msi 文件，应该匹配到并返回 true (由于是 dry_run 不会真实执行安装)
        fs::remove_file(&txt_file).unwrap();
        let msi_file = dir.path().join("installer.msi");
        fs::write(&msi_file, "dummy installer").unwrap();

        let res = check_and_install_msi(dir.path(), true).unwrap();
        assert!(res);
    }

    #[test]
    fn test_remove_shim_artifacts_syncs_fs_and_tracking() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("foo");

        // Pre-track the current shim-rs artifacts (as a previous install would).
        let mut repo = Repo::new("foo");
        for ext in ["exe", "shim"] {
            let p = base.with_extension(ext);
            fs::write(&p, "x").unwrap();
            repo.installed_files.insert(p);
        }
        repo.installed_files.insert(base.clone());
        fs::write(&base, "#!/bin/sh").unwrap();

        // Legacy artifacts from the old (python) bpm (not tracked).
        fs::write(base.with_extension("lnk"), "lnk").unwrap();
        fs::write(base.with_extension("cmd"), "cmd").unwrap();

        repo.remove_shim_artifacts(&base);

        // FS: everything removed (current + legacy).
        for ext in ["exe", "shim", "lnk", "cmd"] {
            assert!(
                !base.with_extension(ext).exists(),
                "{ext} artifact should be removed"
            );
        }
        assert!(!base.exists(), "bare sh script should be removed");

        // Tracking: previously tracked paths are untracked in the same call.
        assert!(repo.installed_files.is_empty());
    }

    #[test]
    fn test_remove_shim_artifacts_ignores_missing() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("nothing-here");
        // No files exist and nothing is tracked — must not error or change state.
        let mut repo = Repo::new("foo");
        repo.remove_shim_artifacts(&base);
        assert!(repo.installed_files.is_empty());
    }
}
