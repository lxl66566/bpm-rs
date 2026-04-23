use std::path::PathBuf;

use crate::{error::BpmResult, storage::db::DbOperation};

#[derive(Debug)]
pub struct Context {
    pub dry_run: bool,
    pub quiet: bool,
    install_position: PathBuf,
    db_path: PathBuf,
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    pub fn with_install_position(mut self, path: impl Into<PathBuf>) -> Self {
        self.install_position = path.into();
        self
    }

    pub fn with_db_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.db_path = path.into();
        self
    }

    pub fn db(&self) -> BpmResult<crate::storage::db::Db> {
        Ok(crate::storage::db::Db::create_or_open(&self.db_path)?)
    }

    #[inline]
    pub fn app_path(&self) -> PathBuf {
        self.install_position.join("app")
    }

    #[inline]
    pub fn bin_path(&self) -> PathBuf {
        self.install_position.join("bin")
    }
}

impl Default for Context {
    fn default() -> Self {
        let home = home::home_dir().expect("Failed to get home directory");
        Self {
            dry_run: false,
            quiet: false,
            install_position: home.join(".bpm"),
            db_path: home.join(".config").join("bpm").join("db"),
        }
    }
}
