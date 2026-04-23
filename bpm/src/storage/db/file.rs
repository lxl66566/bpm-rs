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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Repo;

    #[test]
    fn test_db_basic_operation() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Db::create_or_open(temp_dir.path().join("test.ron"))?;
        db.insert_repo(Repo::new("bpm").by_url("https://github.com/lxl66566/bpm-rs/"))?;
        db.insert_repo(Repo::new("abd").by_url("https://github.com/lxl6656645/b132/"))?;
        let all = db.get_repo_list();
        assert_eq!(all.len(), 2);
        Ok(())
    }
}
