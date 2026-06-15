use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use log::{debug, info, warn};
use walkdir::WalkDir;

use super::Installation;
use crate::{
    context::Context,
    storage::Repo,
    utils::path::PathExt,
};

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

/// Determines installation target paths based on whether the user is root.
pub struct UnixPaths {
    root: PathBuf,
}

impl UnixPaths {
    #[must_use]
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
        Self::new()
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
        let paths = UnixPaths::new();
        assert!(paths.bin().ends_with("bin"));
        assert!(paths.lib().ends_with("lib"));
        assert!(paths.share().ends_with("share"));
        assert!(paths.include().ends_with("include"));
    }
}
