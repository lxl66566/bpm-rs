use std::{
    cell::OnceCell,
    path::{Path, PathBuf},
    sync::LazyLock as Lazy,
};

use crate::storage::db::{Db, DbOperation};
use die_exit::{Die, DieWith};
use home::home_dir;
use serde::{Deserialize, Serialize};

pub static CONFIG_POSITION: Lazy<PathBuf> = Lazy::new(|| {
    home_dir()
        .die("Failed to get home directory.")
        .join(".config")
        .join("bpm")
});

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub install_position: PathBuf,
    pub cache_position: PathBuf,
    pub db_path: PathBuf,
    #[serde(skip)]
    db: OnceCell<Db>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            install_position: home_dir().die("Failed to get home directory.").join(".bpm"),
            cache_position: std::env::temp_dir().join(".bpm"),
            db_path: CONFIG_POSITION.join("db"),
            db: OnceCell::new(),
        }
    }
}

impl Config {
    pub fn db(&self) -> &Db {
        self.db.get_or_init(|| {
            Db::create_or_open(&self.db_path).die_with(|e| {
                format!(
                    "Failed to create or open database in {} : {e}",
                    self.db_path.display()
                )
            })
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
