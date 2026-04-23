pub mod download;
pub mod unzip;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use log::{info, warn};

use crate::{context::Context, utils::path::PathExt};

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

fn move_files_recursively<F>(src_dir: &Path, dst_dir: &Path, dry_run: bool, f: &mut F) -> Result<()>
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
            move_files_recursively(&src_path, &dst_path, dry_run, f)?;
            if !dry_run {
                let _ = fs::remove_dir(&src_path);
            }
            info!("Moving dir : {src_path:?} -> {dst_path:?}");
            f(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            if !dry_run {
                fs::rename(&src_path, &dst_path)?;
            }
            info!("Moving file: {src_path:?} -> {dst_path:?}");
            f(&src_path, &dst_path)?;
        } else {
            warn!("Skipping non-file & non-directory: {src_path:?}");
        }
    }
    Ok(())
}

#[cfg(unix)]
fn rename_old(path: &Path) -> std::io::Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let new_ext = format!("{ext}.old");
    let old = path.with_extension(new_ext);
    fs::rename(path, old)
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
        && file.extension() == Some(std::ffi::OsStr::new("msi"))
    {
        info!("Start to install msi file {}.", file.display());
        if dry_run {
            info!("Dry run, skip installation.");
            return Ok(true);
        }
        let mut command = std::process::Command::new("msiexec.exe");
        command.arg("/i");
        command.arg(&file);
        command.arg("/quiet");
        command.arg("/qr");
        command.arg("/norestart");
        command.status()?;
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
                home::home_dir()
                    .expect("Failed to get home directory")
                    .join(".config")
                    .join("systemd")
                    .join("user")
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
        if dry_run {
            info!("dry run: {dst:?}");
            recorder.push(dst.to_path_buf());
            return Ok(());
        }

        if src.is_dir() {
            fs::create_dir_all(dst)?;
            info!("mkdir {src:?} -> {dst:?}");
            recorder.push(dst.to_path_buf());
            return Ok(());
        }

        if dst.exists() {
            rename_old(dst)?;
        }

        fs::copy(src, dst)?;
        info!("{src:?} -> {dst:?}");
        recorder.push(dst.to_path_buf());

        if let Some(m) = mode {
            fs::set_permissions(dst, fs::Permissions::from_mode(m))?;
        }
        Ok(())
    }

    fn merge_dir(from: &Path, to: &Path, dry_run: bool, recorder: &mut Vec<PathBuf>) -> Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let src = entry.path();
            let dst = to.join(entry.file_name());
            if src.is_dir() {
                merge_dir(&src, &dst, dry_run, recorder)?;
            } else {
                install_file(&src, &dst, dry_run, None, recorder)?;
            }
        }
        Ok(())
    }

    fn install_completions(
        path: &Path,
        unix_paths: &UnixPaths,
        dry_run: bool,
        recorder: &mut Vec<PathBuf>,
    ) -> Result<()> {
        if !path.is_dir() {
            warn!("trying to install {path:?} as completions: not a directory");
            return Ok(());
        }
        debug!("installing completions from {path:?}");

        for entry in walk_files(path, "*.fish") {
            let dst = unix_paths
                .share()
                .join("fish")
                .join("vendor_completions.d")
                .join(entry.file_name().unwrap());
            install_file(&entry, &dst, dry_run, Some(0o644), recorder)?;
        }

        for entry in walk_files(path, "*.bash") {
            let dst = unix_paths
                .share()
                .join("bash-completion")
                .join("completions")
                .join(entry.file_name().unwrap());
            install_file(&entry, &dst, dry_run, Some(0o644), recorder)?;
        }

        for entry in walk_glob(path, "_*") {
            if entry.is_file() {
                if let Ok(content) = fs::read_to_string(&entry) {
                    if content.contains("zsh") {
                        let dst = unix_paths
                            .share()
                            .join("zsh")
                            .join("site-functions")
                            .join(entry.file_name().unwrap());
                        install_file(&entry, &dst, dry_run, Some(0o644), recorder)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn walk_files(dir: &Path, pattern: &str) -> Vec<PathBuf> {
        let mut results = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    results.extend(walk_files(&path, pattern));
                } else if let Some(name) = path.file_name() {
                    if let Some(name_str) = name.to_str() {
                        if name_str.ends_with(pattern.trim_start('*')) {
                            results.push(path);
                        }
                    }
                }
            }
        }
        results
    }

    fn walk_glob(dir: &Path, pattern: &str) -> Vec<PathBuf> {
        let prefix = pattern.trim_start_matches('_');
        let mut results = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    results.extend(walk_glob(&path, pattern));
                } else if let Some(name) = path.file_name() {
                    if let Some(name_str) = name.to_str() {
                        if name_str.starts_with('_') {
                            results.push(path);
                        }
                    }
                }
            }
        }
        results
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
    use std::{fs, path::Path};

    use anyhow::Result;
    use log::{info, warn};

    use super::{check_and_install_msi, move_files_recursively};
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
            app_dir.create_dir_if_not_exist()?;
            self.installed_files.push(app_dir.clone());

            move_files_recursively(src, &app_dir, ctx.dry_run, &mut |_src, dst| {
                self.installed_files.push(dst.to_path_buf());
                Ok(())
            })?;

            self.create_binary_links(ctx)?;
            Ok(())
        }

        fn uninstall(&mut self, ctx: &Context) -> Result<()> {
            if ctx.dry_run {
                for file in &self.installed_files {
                    info!("dry run: Remove file: {file:?}");
                }
                return Ok(());
            }

            let install_position = ctx.app_path();
            let install_parent = install_position.parent().unwrap_or(Path::new("."));
            for file in &self.installed_files {
                if !file.is_subpath_of(install_parent) {
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
            use mslnk::ShellLink;
            use path_absolutize::Absolutize;

            use crate::utils::path::{windows_path_to_windows_bash, windows_path_to_wsl};

            let app_dir = ctx.app_path().join(&self.name);
            let bin_path = ctx.bin_path();
            bin_path.create_dir_if_not_exist()?;

            let bin_files = app_dir.glob_name(&self.bin_name);
            if bin_files.is_empty() {
                warn!("No binary file found in {app_dir:?}");
                return Ok(());
            }

            let display_names: Vec<String> = bin_files
                .iter()
                .map(|x| {
                    format!(
                        "`{}`",
                        x.with_extension("").file_name().unwrap().to_str().unwrap()
                    )
                })
                .collect();

            for bin_file in &bin_files {
                let stem = bin_file
                    .with_extension("")
                    .file_name()
                    .unwrap()
                    .to_os_string()
                    .into_string()
                    .unwrap();
                let lnk_path = bin_path.join(&stem).with_extension("lnk");
                let cmd_path = bin_path.join(&stem).with_extension("cmd");
                let sh_path = bin_path.join(&stem);

                if ctx.dry_run {
                    info!("dry run: Create lnk: {bin_file:?} -> {lnk_path:?}");
                    info!("dry run: Create cmd: {bin_file:?} -> {cmd_path:?}");
                    info!("dry run: Create sh: {bin_file:?} -> {sh_path:?}");
                    continue;
                }

                if lnk_path.exists() {
                    let _ = fs::remove_file(&lnk_path);
                }
                if cmd_path.exists() {
                    let _ = fs::remove_file(&cmd_path);
                }
                if sh_path.exists() {
                    let _ = fs::remove_file(&sh_path);
                }

                let sl = ShellLink::new(bin_file)?;
                sl.create_lnk(&lnk_path)?;
                info!("Create lnk: {bin_file:?} -> {lnk_path:?}");
                self.installed_files.push(lnk_path);

                let cmd_content =
                    format!("@echo off\r\n\"{}\" %*", bin_file.absolutize()?.display());
                fs::write(&cmd_path, cmd_content)?;
                info!("Create cmd: {bin_file:?} -> {cmd_path:?}");
                self.installed_files.push(cmd_path);

                let sh_content = format!(
                    "#!/bin/sh\nif [ \"$(uname)\" != \"Linux\" ]; then\n    \"{}\" \"$@\"\nelse\n    \"{}\" \"$@\"\nfi",
                    windows_path_to_windows_bash(bin_file),
                    windows_path_to_wsl(bin_file)
                );
                fs::write(&sh_path, sh_content)?;
                info!("Create sh: {bin_file:?} -> {sh_path:?}");
                self.installed_files.push(sh_path);
            }

            info!("Successfully installed `{}`.", self.name);
            info!(
                "You can press `Win+r`, enter {} to start software, or execute in cmd.",
                display_names.join(", ")
            );

            ensure_windows_path(&bin_path);
            Ok(())
        }
    }

    fn ensure_windows_path(bin_path: &Path) {
        if let Ok(path_str) = std::env::var("PATH") {
            let path_str_lower = path_str.to_lowercase();
            let bin_str = bin_path.to_str().unwrap_or("");
            if path_str_lower.contains(&bin_str.to_lowercase()) {
                return;
            }
        }
        if let Err(e) = add_to_windows_path(bin_path) {
            warn!("Failed to add {} to PATH: {e}", bin_path.display());
        }
    }

    fn add_to_windows_path(new_path: &Path) -> Result<()> {
        let new_path_str = new_path.to_str().unwrap_or("");
        let output = std::process::Command::new("reg")
            .args(["query", "HKCU\\Environment", "/v", "PATH"])
            .output()?;

        let current_path = if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_reg_path_value(&stdout)
        } else {
            None
        };

        let new_value = match current_path {
            Some(ref cp) => format!("{new_path_str};{cp}"),
            None => new_path_str.to_string(),
        };

        std::process::Command::new("reg")
            .args([
                "add",
                "HKCU\\Environment",
                "/t",
                "REG_SZ",
                "/v",
                "PATH",
                "/d",
                &new_value,
                "/f",
            ])
            .status()?;

        info!("{new_path_str} added to PATH. You may need to relogin to apply.");
        Ok(())
    }

    fn parse_reg_path_value(output: &str) -> Option<String> {
        for line in output.lines() {
            let line = line.trim();
            if line.contains("PATH") && line.contains("REG_") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    return Some(parts[2..].join(""));
                }
            }
        }
        None
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
        crate::utils::log::log_init();

        let src_dir = tempdir()?;
        let src_path = src_dir.path();

        let file1_path = src_path.join("file1.txt");
        File::create(&file1_path)?;

        let file2_path = src_path.join("file2.txt");
        File::create(&file2_path)?;

        let subdir_path = src_path.join("subdir");
        fs::create_dir(&subdir_path)?;

        let subfile1_path = subdir_path.join("subfile1.txt");
        File::create(&subfile1_path)?;

        let dst_dir = tempdir()?;
        let dst_path = dst_dir.path();

        let mut record = vec![];

        move_files_recursively(src_path, dst_path, false, &mut |_, dst| {
            record.push(dst.to_path_buf());
            Ok(())
        })?;

        assert!(dst_path.join("file1.txt").exists());
        assert!(dst_path.join("file2.txt").exists());
        assert!(dst_path.join("subdir").exists());
        assert!(dst_path.join("subdir/subfile1.txt").exists());
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

        assert!(!file1_path.exists());
        assert!(!file2_path.exists());
        assert!(!subfile1_path.exists());
        assert!(!subdir_path.exists());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_rename_old_restore_old() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("test.txt");
        File::create(&file_path)?;

        rename_old(&file_path)?;
        assert!(!file_path.exists());
        assert!(dir.path().join("test.txt.old").exists());

        restore_old(&file_path)?;
        assert!(file_path.exists());
        assert!(!dir.path().join("test.txt.old").exists());
        Ok(())
    }
}
