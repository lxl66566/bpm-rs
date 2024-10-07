use std::{
    path::{Path, PathBuf},
    sync::{LazyLock as Lazy, OnceLock},
};

use die_exit::{Die, DieWith};
use home::home_dir;
use native_db::Database;
use serde::{Deserialize, Serialize};

use crate::storage::{db::DbInit, MODELS};

pub static CONFIG_POSITION: Lazy<PathBuf> = Lazy::new(|| {
    home_dir()
        .die("Failed to get home directory.")
        .join(".config")
        .join("bpm")
});

#[derive(Serialize, Deserialize)]
pub struct Config<'a> {
    pub install_position: PathBuf,
    pub cache_position: PathBuf,
    pub db_position: PathBuf,
    #[serde(skip)]
    db: OnceLock<Database<'a>>,
}

impl Default for Config<'_> {
    fn default() -> Self {
        Self {
            install_position: home_dir().die("Failed to get home directory.").join(".bpm"),
            cache_position: std::env::temp_dir().join(".bpm"),
            db_position: CONFIG_POSITION.join("db"),
            db: OnceLock::new(),
        }
    }
}

impl Config<'_> {
    pub fn db(&self) -> &Database<'_> {
        self.db.get_or_init(|| {
            let db = native_db::Builder::new()
                .create_or_open(&MODELS, &self.db_position)
                .die_with(|e| {
                    format!(
                        "Failed to create or open database in {} : {e}",
                        self.db_position.display()
                    )
                });
            db
        })
    }
    #[inline]
    pub fn app_path(&self) -> PathBuf {
        self.install_position.join("app")
    }
    #[inline]
    pub fn bin_path(&self) -> PathBuf {
        self.install_position.join("bin")
    }
    #[inline]
    pub fn cache_path(&self) -> &Path {
        self.cache_position.as_path()
    }
}
