use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use log::{debug, info, warn};
use walkdir::WalkDir;

use super::Installation;
use crate::{context::Context, storage::Repo, utils::path::PathExt};

/// Rename an existing file to `<name>.old` to avoid overwriting.
pub fn rename_old(path: &Path) -> std::io::Result<()> {
    let new_path = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => path.with_extension(format!("{ext}.old")),
        None => path.with_extension("old"),
    };
    fs::rename(path, new_path)
}

/// Restore a `<name>.old` file back to its original name.
pub fn restore_old(path: &Path) -> std::io::Result<()> {
    let old_path = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => path.with_extension(format!("{ext}.old")),
        None => path.with_extension("old"),
    };
    if old_path.exists() {
        fs::rename(&old_path, path)?;
        info!("Restoring {old_path:?} -> {path:?}");
    }
    Ok(())
}

/// Determines installation target paths based on whether the user is root
/// or a custom prefix is provided.
pub struct UnixPaths {
    root: PathBuf,
}

impl UnixPaths {
    #[must_use]
    pub fn new(prefix: Option<&Path>) -> Self {
        let root = match prefix {
            Some(p) => p.to_path_buf(),
            None => {
                if crate::utils::is_root() {
                    PathBuf::from("/usr")
                } else {
                    home::home_dir()
                        .expect("Failed to get home directory")
                        .join(".local")
                }
            }
        };
        Self { root }
    }

    /// The root prefix for all installation targets (e.g. /usr, ~/.local, or
    /// custom).
    #[inline]
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[inline]
    #[must_use]
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    #[inline]
    #[must_use]
    pub fn lib(&self) -> PathBuf {
        self.root.join("lib")
    }

    #[inline]
    #[must_use]
    pub fn share(&self) -> PathBuf {
        self.root.join("share")
    }

    #[inline]
    #[must_use]
    pub fn include(&self) -> PathBuf {
        self.root.join("include")
    }

    #[inline]
    #[must_use]
    pub fn services(&self) -> PathBuf {
        if crate::utils::is_root() {
            PathBuf::from("/etc/systemd/system")
        } else {
            home::home_dir()
                .expect("Failed to get home directory")
                .join(".config/systemd/user")
        }
    }
}

impl Default for UnixPaths {
    fn default() -> Self {
        Self::new(None)
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

        let Some(shell) = detect_completion_shell(src) else {
            continue;
        };

        let file_name = src.file_name().unwrap();
        let dst = match shell {
            Shell::Fish => paths
                .share()
                .join("fish/vendor_completions.d")
                .join(file_name),
            Shell::Bash => paths
                .share()
                .join("bash-completion/completions")
                .join(file_name),
            Shell::Zsh => paths.share().join("zsh/site-functions").join(file_name),
        };

        install_file(src, &dst, dry_run, Some(0o644), recorder)?;
    }
    Ok(())
}

/// The supported shell completion kinds, each mapped to a dedicated install
/// directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Fish,
    Bash,
    Zsh,
}

impl Shell {
    /// Lower-cased directory names that strongly indicate a shell completion
    /// layout (e.g. `completions/bash/<binary>`).
    #[must_use]
    fn from_dir_name(name: &str) -> Option<Self> {
        match name {
            "fish" => Some(Self::Fish),
            "bash" | "bash-completion" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            _ => None,
        }
    }
}

/// Detect the shell a completion file belongs to.
///
/// Detection order (most precise first):
/// 1. The immediate parent directory name (`bash`, `fish`, `zsh`, ...) — this
///    recognizes the common `completions/<shell>/<binary>` layout where files
///    have no distinguishing extension.
/// 2. The file extension (`.fish`, `.bash`).
/// 3. For files prefixed with `_`, a real zsh completion is confirmed by the
///    canonical `#compdef` marker. Only the first bytes are read so large or
///    binary files are not slurped into memory, and the check itself is far
///    more precise than the previous loose "contains zsh" heuristic.
#[must_use]
fn detect_completion_shell(src: &Path) -> Option<Shell> {
    // 1. Parent directory name.
    if let Some(shell) = src
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .and_then(Shell::from_dir_name)
    {
        return Some(shell);
    }

    let name = src.file_name()?.to_string_lossy();

    // 2. File extension.
    if name.ends_with(".fish") {
        return Some(Shell::Fish);
    }
    if name.ends_with(".bash") {
        return Some(Shell::Bash);
    }

    // 3. zsh: `_`-prefixed files verified via the `#compdef` marker.
    if name.starts_with('_') && is_zsh_completion(src) {
        return Some(Shell::Zsh);
    }

    None
}

/// Whether `src` looks like a real zsh completion by checking the canonical
/// `#compdef` directive. Only the leading bytes are inspected so huge or
/// binary files are never fully read.
#[must_use]
fn is_zsh_completion(src: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = fs::File::open(src) else {
        return false;
    };
    let mut head = [0u8; 512];
    let n = file.read(&mut head).unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..n]);
    head.contains("#compdef")
}

impl Installation for Repo {
    fn install(&mut self, src: impl AsRef<Path>, ctx: &Context) -> Result<()> {
        let src = src.as_ref();
        let unix_paths = UnixPaths::new(ctx.prefix());
        let dry_run = ctx.dry_run;
        let first_layer: Vec<_> = std::fs::read_dir(src)?
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
                let candidates = src.glob_name(&self.bin_name);
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
                "usr" => merge_dir(file, unix_paths.root(), dry_run, &mut self.installed_files)?,
                "lib" => merge_dir(file, &unix_paths.lib(), dry_run, &mut self.installed_files)?,
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
                "bin" => merge_dir(file, &unix_paths.bin(), dry_run, &mut self.installed_files)?,
                "man" => merge_dir(
                    file,
                    &unix_paths.share().join("man"),
                    dry_run,
                    &mut self.installed_files,
                )?,
                // Locale data (e.g. <lang>/LC_MESSAGES/<pkg>.mo) lives under
                // share/locale on Linux.
                "locale" => merge_dir(
                    file,
                    &unix_paths.share().join("locale"),
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

        for service_file in src.glob_name(".service") {
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
            .any(|f| f.to_str().is_some_and(|s| s.contains("bin")));
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
            restore_old(file)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn test_rename_and_restore_old() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("binary.bin");
        let old_path = dir.path().join("binary.bin.old");

        fs::write(&file_path, "executable code").unwrap();
        assert!(file_path.exists());
        assert!(!old_path.exists());

        rename_old(&file_path).unwrap();
        assert!(!file_path.exists(), "Original file should not exist");
        assert!(old_path.exists(), ".old file should exist");

        restore_old(&file_path).unwrap();
        assert!(file_path.exists(), "Original file should be restored");
        assert!(!old_path.exists(), ".old file should be removed");
    }

    #[test]
    fn test_unix_paths_generation() {
        let paths = UnixPaths::new(None);
        assert!(paths.bin().ends_with("bin"));
        assert!(paths.lib().ends_with("lib"));
        assert!(paths.share().ends_with("share"));
        assert!(paths.include().ends_with("include"));
    }

    #[test]
    fn test_detect_completion_shell_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let fish = root.join("foo.fish");
        let bash = root.join("foo.bash");
        fs::write(&fish, "").unwrap();
        fs::write(&bash, "").unwrap();

        assert_eq!(detect_completion_shell(&fish), Some(Shell::Fish));
        assert_eq!(detect_completion_shell(&bash), Some(Shell::Bash));
    }

    #[test]
    fn test_detect_completion_shell_by_parent_dir() {
        // Nested layout: completions/<shell>/<binary> without extensions.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let bash_file = root.join("bash").join("foo");
        let fish_file = root.join("fish").join("foo");
        let zsh_file = root.join("zsh").join("_foo");
        fs::create_dir_all(bash_file.parent().unwrap()).unwrap();
        fs::create_dir_all(fish_file.parent().unwrap()).unwrap();
        fs::create_dir_all(zsh_file.parent().unwrap()).unwrap();
        fs::write(&bash_file, "").unwrap();
        fs::write(&fish_file, "").unwrap();
        fs::write(&zsh_file, "#compdef foo\n").unwrap();

        // Parent directory detection takes precedence over extension checks.
        assert_eq!(detect_completion_shell(&bash_file), Some(Shell::Bash));
        assert_eq!(detect_completion_shell(&fish_file), Some(Shell::Fish));
        assert_eq!(detect_completion_shell(&zsh_file), Some(Shell::Zsh));
    }

    #[test]
    fn test_detect_zsh_requires_compdef_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Real zsh completion: starts with `_` and has `#compdef`.
        let real = root.join("_foo");
        fs::write(&real, "#compdef foo\n# the rest...\n").unwrap();
        assert!(is_zsh_completion(&real));
        assert_eq!(detect_completion_shell(&real), Some(Shell::Zsh));

        // Imposter: starts with `_` and mentions "zsh" but has no `#compdef`.
        // The old heuristic ("contains zsh") wrongly accepted this; the new
        // check must reject it.
        let imposter = root.join("_bar");
        fs::write(&imposter, "echo zsh\n").unwrap();
        assert!(!is_zsh_completion(&imposter));
        assert_eq!(detect_completion_shell(&imposter), None);
    }

    #[test]
    fn test_detect_completion_rejects_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let readme = root.join("README.md");
        let underscore = root.join("_notes"); // no #compdef
        fs::write(&readme, "docs").unwrap();
        fs::write(&underscore, "notes").unwrap();

        assert_eq!(detect_completion_shell(&readme), None);
        assert_eq!(detect_completion_shell(&underscore), None);
    }

    #[test]
    fn test_install_completions_records_correct_paths() {
        // Flat layout under a `completions`-like dir.
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path();

        fs::write(src.join("foo.fish"), "").unwrap();
        fs::write(src.join("foo.bash"), "").unwrap();
        fs::write(src.join("_foo"), "#compdef foo\n").unwrap();
        // Unrelated file must be skipped.
        fs::write(src.join("README.md"), "ignore me").unwrap();

        let tmp_root = tempfile::tempdir().unwrap();
        let paths = UnixPaths::new(Some(tmp_root.path()));
        let mut recorder = Vec::new();

        install_completions(src, &paths, true, &mut recorder).unwrap();

        let share = tmp_root.path().join("share");
        let expected = [
            share.join("fish/vendor_completions.d/foo.fish"),
            share.join("bash-completion/completions/foo.bash"),
            share.join("zsh/site-functions/_foo"),
        ];
        for path in &expected {
            assert!(
                recorder.iter().any(|p| p == path),
                "expected {path:?} in recorder: {recorder:?}"
            );
        }
        // README must not leak into the recorded completions.
        assert!(
            !recorder
                .iter()
                .any(|p| p.file_name().and_then(|n| n.to_str()) == Some("README.md"))
        );
    }

    #[test]
    fn test_install_merges_locale_folder() {
        // Build a fake package: a binary (so the multi-file branch runs) and a
        // locale/ tree that should be merged into share/locale.
        let pkg = tempfile::tempdir().unwrap();
        let src = pkg.path();

        let bin = src.join("mybin");
        fs::write(&bin, "#!/bin/sh\n").unwrap();

        let mo = src
            .join("locale")
            .join("en")
            .join("LC_MESSAGES")
            .join("mybin.mo");
        fs::create_dir_all(mo.parent().unwrap()).unwrap();
        fs::write(&mo, "translated").unwrap();

        let tmp_root = tempfile::tempdir().unwrap();
        let ctx = Context::new()
            .with_dry_run(true)
            .with_prefix(Some(tmp_root.path().to_path_buf()));

        let mut repo = Repo::new("mybin");
        repo.bin_name = "mybin".into();

        repo.install(src, &ctx).unwrap();

        let expected_mo = tmp_root
            .path()
            .join("share")
            .join("locale")
            .join("en")
            .join("LC_MESSAGES")
            .join("mybin.mo");
        assert!(
            repo.installed_files.iter().any(|p| p == &expected_mo),
            "locale file should be merged into share/locale, got: {:#?}",
            repo.installed_files
        );
    }
}
