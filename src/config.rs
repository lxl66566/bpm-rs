//! User configuration file (`--config <path>`, default `~/.config/bpm/config.toml`).
//!
//! All fields are optional; missing file / fields fall back to built-in
//! defaults. Precedence: CLI flags > config file > defaults. The GitHub
//! token additionally yields to `GITHUB_TOKEN` / `GH_TOKEN` env vars.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Result, ensure};
use config_file2::LoadConfigFile;
use serde::{Deserialize, Serialize};

/// Config file this process actually uses; set once from the CLI before the
/// HTTP client lazily initializes (it reads the token from here).
static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Root directory for installed apps and shims (Windows layout).
    /// Moving this also moves the db unless `db_path` is set.
    pub install_position: Option<PathBuf>,
    /// Package database file path.
    pub db_path: Option<PathBuf>,
    /// Unix install prefix (e.g. `/usr/local`); overridden by `--prefix`.
    pub prefix: Option<PathBuf>,
    /// GitHub API token; env vars `GITHUB_TOKEN` / `GH_TOKEN` take priority.
    pub github_token: Option<String>,
}

impl Config {
    /// Default config location.
    #[must_use]
    pub fn default_path() -> PathBuf {
        home::home_dir()
            .unwrap_or_default()
            .join(".config")
            .join("bpm")
            .join("config.toml")
    }

    /// Resolve the config path: an explicit path must exist (a typo should
    /// fail loudly, not silently fall back); the default path may be absent.
    pub fn resolve_path(explicit: Option<&Path>) -> Result<PathBuf> {
        match explicit {
            Some(p) => {
                ensure!(p.is_file(), "config file not found: {}", p.display());
                Ok(p.to_path_buf())
            },
            None => Ok(Self::default_path()),
        }
    }

    /// Record the resolved config path for this process (used by the HTTP
    /// client to find `github_token`). No-op after the first call.
    pub fn set_active_path(path: PathBuf) {
        let _ = ACTIVE_PATH.set(path);
    }

    /// GitHub token: env first (easy temporary override), then config file.
    /// Returns `None` when neither is set / loadable.
    #[must_use]
    pub fn github_token() -> Option<String> {
        if let Ok(token) = std::env::var("GITHUB_TOKEN")
            && !token.is_empty()
        {
            return Some(token);
        }
        if let Ok(token) = std::env::var("GH_TOKEN")
            && !token.is_empty()
        {
            return Some(token);
        }
        let path = ACTIVE_PATH
            .get()
            .cloned()
            .unwrap_or_else(Self::default_path);
        // A missing file or malformed file yields Err -> no token (optional).
        Self::load(&path).ok()?.github_token
    }

    /// Load from a path, treating a missing file as defaults.
    /// Inherent wrapper so callers do not need the `LoadConfigFile` trait.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Ok(LoadConfigFile::load_or_default(path.as_ref())?)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolve_path_explicit_must_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope.toml");
        assert!(Config::resolve_path(Some(&missing)).is_err());

        let existing = tmp.path().join("yes.toml");
        fs::write(&existing, "").unwrap();
        assert_eq!(Config::resolve_path(Some(&existing)).unwrap(), existing);
    }

    #[test]
    fn load_partial_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        fs::write(
            &path,
            "install_position = \"/custom/bpm\"\ngithub_token = \"ghp_test\"\n",
        )
        .unwrap();

        let config = Config::load(&path).unwrap();
        assert_eq!(
            config.install_position.as_deref(),
            Some(Path::new("/custom/bpm"))
        );
        assert_eq!(config.github_token.as_deref(), Some("ghp_test"));
        // Unset fields default to None.
        assert!(config.prefix.is_none());
        assert!(config.db_path.is_none());
    }

    #[test]
    fn missing_file_yields_defaults() {
        let config = Config::load("/nonexistent/config.toml").unwrap();
        assert_eq!(config, Config::default());
    }
}
