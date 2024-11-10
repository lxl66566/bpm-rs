//! use `config-file2` as the backend of the storage.

use crate::storage::{Repo, RepoList};

use super::DbOperation;
use config_file2::{LoadConfigFile, StoreConfigFile};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};

#[derive(Debug, Serialize, Deserialize)]
pub struct Db {
    pub db_path: PathBuf,
    pub repo_list: Mutex<RepoList>,
}

impl DbOperation for Db {
    fn create_or_open(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let db_path = path.as_ref().to_path_buf();
        let repo_list = RepoList::load_or_default(&db_path)?.into();
        Ok(Self { db_path, repo_list })
    }

    fn get_repo_list(&self) -> RepoList {
        self.repo_list.lock().unwrap().clone()
    }

    fn get_repo(&self, name: &str) -> Option<Repo> {
        self.repo_list
            .lock()
            .unwrap()
            .iter()
            .find(|x| x.name == name)
            .cloned()
    }

    fn insert_repo(&self, repo: Repo) -> anyhow::Result<()> {
        self.repo_list.lock().unwrap().push(repo);
        self.repo_list.store(&self.db_path)?;
        Ok(())
    }

    fn remove_repo(&self, name: &str) -> anyhow::Result<()> {
        self.repo_list.lock().unwrap().retain(|x| x.name != name);
        self.repo_list.store(&self.db_path)?;
        Ok(())
    }
}
