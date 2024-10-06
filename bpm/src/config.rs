use die_exit::Die;
use home::home_dir;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Config {
    pub install_position: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            install_position: home_dir().die("Failed to get home directory.").join(".bpm"),
        }
    }
}
