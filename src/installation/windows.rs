use std::{
    fs::{self, File},
    io::Cursor,
    path::{Path, PathBuf},
};

use anyhow::{Result, ensure};
use log::{debug, info, warn};
use walkdir::WalkDir;

use super::Installation;
use crate::{context::Context, storage::Repo, utils::path::PathExt};

// File moves go through fs_extra, which supports cross-device moves.
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
    options.content_only = true; // move contents only, not the outer folder itself
    options.overwrite = true;

    // move_dir handles same-device rename and cross-device copy+delete.
    fs_extra::dir::move_dir(src_dir, dst_dir, &options)?;
    info!(
        "Moved contents: `{:?}` -> `{:?}`",
        src_dir.display(),
        dst_dir.display()
    );
    Ok(())
}

/// Find the MSI installer among the extraction result, if any.
///
/// A single `.msi` used to be recognized only when it was the *sole* file;
/// packages shipping `foo.msi + readme.txt` fell through to the regular
/// install path and were moved as data files. Any top-level `.msi` counts now.
fn find_msi(src: &Path) -> std::io::Result<Option<PathBuf>> {
    let mut msis: Vec<PathBuf> = fs::read_dir(src)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("msi"))
        })
        .collect();
    // Deterministic pick when several MSIs ship together (e.g. x86/x64
    // variants); bpm cannot tell the right one apart.
    msis.sort();
    if msis.len() > 1 {
        warn!("multiple msi files found: {msis:?}, using the first one");
    }
    Ok(msis.into_iter().next())
}

fn check_and_install_msi(src: impl AsRef<Path>, dry_run: bool) -> Result<bool> {
    let Some(file) = find_msi(src.as_ref())? else {
        return Ok(false);
    };
    info!("Start to install msi file {}.", file.display());
    if dry_run {
        info!("Dry run, skip installation.");
        return Ok(true);
    }
    // /quiet and /qr contradict each other (fully silent vs reduced UI);
    // use /quiet only.
    let output = std::process::Command::new("msiexec.exe")
        .args([
            "/i",
            file.to_str().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "msi path is not UTF-8")
            })?,
            "/quiet",
            "/norestart",
        ])
        .status()
        .map_err(|e| anyhow::anyhow!("failed to spawn msiexec.exe: {e}"))?;
    // A non-zero msiexec exit code means the installation failed; the
    // previous code treated a spawned-but-failed install as success.
    ensure!(
        output.success(),
        "msiexec exited with {} for {}",
        output.code().unwrap_or(-1),
        file.display()
    );
    Ok(true)
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
                "MSI package '{}' was installed silently via msiexec. bpm cannot track or \
                 uninstall MSI-installed files — use Windows Settings > Apps to uninstall.",
                self.name
            );
            self.is_msi = true;
            // The repo record (with is_msi set) is committed by the caller's
            // batch commit, keeping the two-phase "install all, then persist"
            // design. Writing to the DB here would bypass rollback handling
            // and open a second, possibly stale, Db instance.
            return Ok(());
        }

        let app_dir = ctx.app_path().join(&self.name);

        // Move the directory as a whole; individual files inside are not
        // tracked.
        self.add_installed_file(&app_dir, || move_dir_content(src, &app_dir, ctx.dry_run))?;

        self.create_binary_links(src, ctx)?;
        Ok(())
    }

    fn uninstall(&mut self, ctx: &Context) -> Result<()> {
        if self.is_msi {
            warn!(
                "`{}` was installed from an MSI package; its files cannot be tracked. Uninstall \
                 it via Windows Settings > Apps.",
                self.name
            );
        }
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
    fn create_binary_links(&mut self, src: &Path, ctx: &Context) -> Result<()>;
}

impl WindowsBinaryLinks for Repo {
    fn create_binary_links(&mut self, src: &Path, ctx: &Context) -> Result<()> {
        use path_absolutize::Absolutize;

        use crate::utils::path::{windows_path_to_windows_bash, windows_path_to_wsl};

        debug!("creating binary links for {self:?}");

        let app_dir = ctx.app_path().join(&self.name);
        let bin_path = ctx.bin_path();

        // Dry-run leaves app_dir unpopulated (the move is skipped), so scan
        // the extract source instead; scanning app_dir would always produce a
        // misleading "no binary file found" warning.
        let scan_root: &Path = if ctx.dry_run {
            src
        } else {
            &app_dir
        };

        let mut bin_files = scan_root.glob_name(&self.bin_name);
        if bin_files.is_empty() {
            // Fallback: scan for any .exe file.
            bin_files = WalkDir::new(scan_root)
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
                scan_root.display()
            );
            return Ok(());
        }

        if !ctx.dry_run {
            // Both are real side effects (a created directory and a
            // persistent user-PATH modification) and must never run on
            // dry-run.
            bin_path.create_dir_if_not_exist()?;
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
                    let bash_path = windows_path_to_windows_bash(bin_file)?;
                    let wsl_path = windows_path_to_wsl(bin_file)?;
                    fs::write(
                        &sh_path,
                        format!(
                            "#!/bin/sh\nif [ \"$(uname)\" != \"Linux\" ]; then\n    \
                             \"{bash_path}\" \"$@\"\nelse\n    \"{wsl_path}\" \"$@\"\nfi"
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

            ensure_windows_path(&bin_path);
        }
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

    // Skip persistent PATH modification for temp/test directories to avoid
    // polluting the system PATH with temporary entries.
    let temp_dir = std::env::temp_dir();
    if bin_path.starts_with(&temp_dir) {
        debug!("skip adding temp path '{bin_str}' to PATH");
        return;
    }

    // Compare against each PATH entry exactly (ignoring case and a trailing
    // separator): a substring check would also match sibling dirs like
    // `...\bpm\bin2`.
    if let Ok(path_str) = std::env::var("PATH") {
        let already = path_str.split(';').any(|entry| {
            let entry = entry.trim_end_matches('\\');
            let bin = bin_str.trim_end_matches('\\');
            entry.eq_ignore_ascii_case(bin)
        });
        if already {
            return;
        }
    }
    match windows_env::append("PATH", &bin_str) {
        Ok(()) => {
            info!("{bin_str} added to PATH. You may need to restart the terminal to apply.");
        },
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

        // Source directory layout:
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

        // No files at all -> false.
        assert!(!check_and_install_msi(dir.path(), true).unwrap());

        // A single non-msi file -> false.
        fs::write(dir.path().join("test.txt"), "dummy").unwrap();
        assert!(!check_and_install_msi(dir.path(), true).unwrap());

        // A sole msi -> matched (dry_run skips the actual msiexec call).
        fs::remove_file(dir.path().join("test.txt")).unwrap();
        fs::write(dir.path().join("installer.msi"), "dummy installer").unwrap();
        assert!(check_and_install_msi(dir.path(), true).unwrap());

        // msi + extra files -> still matched; a sole-msi requirement used to
        // send such packages down the regular install path.
        fs::write(dir.path().join("readme.txt"), "readme").unwrap();
        assert!(check_and_install_msi(dir.path(), true).unwrap());

        // Multiple msis -> the deterministically first one is used.
        fs::write(dir.path().join("aaa.msi"), "dummy installer").unwrap();
        assert!(check_and_install_msi(dir.path(), true).unwrap());
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
