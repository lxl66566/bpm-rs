use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::storage::db::DbOperation;

#[derive(Debug)]
pub struct Context {
    pub dry_run: bool,
    pub quiet: bool,
    install_position: PathBuf,
    db_path: PathBuf,
    /// Unix install prefix. None = auto-detect (/usr for root, ~/.local for
    /// non-root).
    prefix: Option<PathBuf>,
}

impl Default for Context {
    fn default() -> Self {
        let home = home::home_dir().expect("Failed to get home directory");
        let install_position = home.join("bpm");
        let db_path = install_position.join("db.json");
        Self {
            dry_run: false,
            quiet: false,
            install_position,
            db_path,
            prefix: None,
        }
    }
}

impl Context {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a context from a loaded [`Config`] plus an optional CLI
    /// `--prefix` (the CLI flag wins over the config file).
    #[must_use]
    pub fn from_config(config: &crate::config::Config, cli_prefix: Option<PathBuf>) -> Self {
        let mut this = Self::default();
        if let Some(p) = &config.install_position {
            this.install_position = p.clone();
            // db follows install_position unless db_path is set explicitly.
            this.db_path = p.join("db.json");
        }
        if let Some(p) = &config.db_path {
            this.db_path = p.clone();
        }
        this.prefix = cli_prefix.or_else(|| config.prefix.clone());
        this
    }

    #[must_use]
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    #[must_use]
    pub fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    #[must_use]
    pub fn with_install_position(mut self, path: impl Into<PathBuf>) -> Self {
        self.install_position = path.into();
        self
    }

    #[must_use]
    pub fn with_db_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.db_path = path.into();
        self
    }

    /// Set the Unix install prefix (e.g. /usr/local, ~/.local).
    /// When None, the prefix is auto-detected: /usr for root, ~/.local for
    /// non-root.
    #[must_use]
    pub fn with_prefix(mut self, prefix: Option<PathBuf>) -> Self {
        self.prefix = prefix;
        self
    }

    pub fn db(&self) -> Result<crate::storage::db::Db> {
        crate::storage::db::Db::create_or_open(&self.db_path)
    }

    #[inline]
    #[must_use]
    pub fn install_position(&self) -> &Path {
        &self.install_position
    }

    #[inline]
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    #[inline]
    #[must_use]
    pub fn app_path(&self) -> PathBuf {
        self.install_position.join("app")
    }

    #[inline]
    #[must_use]
    pub fn bin_path(&self) -> PathBuf {
        self.install_position.join("bin")
    }

    #[cfg(windows)]
    #[inline]
    #[must_use]
    pub fn shim_exe(&self) -> PathBuf {
        self.install_position.join("shim.exe")
    }

    /// Returns the Unix install prefix if set.
    #[cfg(unix)]
    #[inline]
    #[must_use]
    pub fn prefix(&self) -> Option<&Path> {
        self.prefix.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Repo;

    #[test]
    fn from_config_applies_fields_with_cli_prefix_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let config = crate::config::Config {
            install_position: Some(tmp.path().join("pos")),
            db_path: None,
            prefix: Some(PathBuf::from("/cfg-prefix")),
            github_token: None,
        };

        let ctx = Context::from_config(&config, None);
        assert_eq!(ctx.install_position(), tmp.path().join("pos"));
        // db follows install_position
        assert_eq!(ctx.db_path(), tmp.path().join("pos").join("db.json"));
        #[cfg(unix)]
        assert_eq!(ctx.prefix(), Some(Path::new("/cfg-prefix")));

        // CLI --prefix overrides the config file.
        let ctx = Context::from_config(&config, Some(PathBuf::from("/cli-prefix")));
        assert_eq!(ctx.install_position(), tmp.path().join("pos"));
        #[cfg(unix)]
        assert_eq!(ctx.prefix(), Some(Path::new("/cli-prefix")));

        // Explicit db_path does not follow install_position.
        let config = crate::config::Config {
            db_path: Some(tmp.path().join("elsewhere.ron")),
            ..config.clone()
        };
        let ctx = Context::from_config(&config, None);
        assert_eq!(ctx.db_path(), tmp.path().join("elsewhere.ron"));
    }

    #[test]
    fn context_db_with_custom_path() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Context::new()
            .with_install_position(tmp.path().join("bpm"))
            .with_db_path(tmp.path().join("my_db.ron"));

        let db = ctx.db().unwrap();
        db.insert_repo(Repo::default()).unwrap(); // need to insert a repo to store db
        assert!(tmp.path().join("my_db.ron").exists());

        db.insert_repo(
            Repo::new("ctx-test")
                .by_url("https://github.com/a/b")
                .unwrap(),
        )
        .unwrap();
        assert!(db.get_repo("ctx-test").is_some());
    }
}
