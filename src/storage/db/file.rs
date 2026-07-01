//! use `config-file2` as the backend of the storage.

use std::{path::PathBuf, sync::Mutex};

use config_file2::{LoadConfigFile, StoreConfigFile};
use serde::{Deserialize, Serialize};

use super::DbOperation;
use crate::storage::{Repo, RepoList};

#[derive(Debug, Serialize, Deserialize)]
pub struct Db {
    pub db_path: PathBuf,
    pub repo_list: Mutex<RepoList>,
}

impl Db {
    /// Atomically persist the repo list to disk.
    ///
    /// Writes to a temporary file first, then renames it to the final path.
    /// This prevents DB corruption if the process is killed mid-write.
    fn store_atomic(&self) -> anyhow::Result<()> {
        // Preserve the original extension so config-file2 can detect the format.
        // e.g. db.json -> db.tmp.json -> rename to db.json
        let tmp_path = {
            let stem = self.db_path.file_stem().unwrap_or_default();
            let tmp_name = match self.db_path.extension() {
                Some(ext) => format!("{}.tmp.{}", stem.to_string_lossy(), ext.to_string_lossy()),
                None => format!("{}.tmp", stem.to_string_lossy()),
            };
            self.db_path.with_file_name(tmp_name)
        };
        self.repo_list.lock().unwrap().store(&tmp_path)?;
        std::fs::rename(&tmp_path, &self.db_path)?;
        Ok(())
    }
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
        // Upsert by name so that updating an already-tracked repo overwrites
        // its previous record instead of appending a duplicate entry.
        self.repo_list.lock().unwrap().upsert(repo);
        self.store_atomic()?;
        Ok(())
    }

    fn remove_repo(&self, name: &str) -> anyhow::Result<()> {
        self.repo_list.lock().unwrap().retain(|x| x.name != name);
        self.store_atomic()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Repo;

    fn temp_db_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_db.ron");
        (dir, path)
    }

    #[test]
    fn test_db_basic_operation() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Db::create_or_open(temp_dir.path().join("test.ron"))?;
        db.insert_repo(
            Repo::new("bpm")
                .by_url("https://github.com/lxl66566/bpm-rs/")
                .unwrap(),
        )?;
        db.insert_repo(
            Repo::new("abd")
                .by_url("https://github.com/lxl6656645/b132/")
                .unwrap(),
        )?;

        let all = db.get_repo_list();
        assert_eq!(all.len(), 2);

        let found = db.get_repo("bpm").unwrap();
        assert_eq!(found.name, "bpm");
        assert_eq!(found.repo_owner.unwrap(), "lxl66566");

        assert!(db.get_repo("nonexistent").is_none());

        db.remove_repo("bpm").unwrap();
        assert!(db.get_repo("bpm").is_none());
        assert!(db.get_repo("abd").is_some());
        assert_eq!(db.get_repo_list().len(), 1);
        Ok(())
    }

    #[test]
    fn db_persistence() {
        let (_dir, path) = temp_db_path();

        {
            let db = Db::create_or_open(&path).unwrap();
            db.insert_repo(
                Repo::new("persist-test")
                    .by_url("https://github.com/test/repo")
                    .unwrap(),
            )
            .unwrap();
        }

        let db2 = Db::create_or_open(&path).unwrap();
        let found = db2.get_repo("persist-test").unwrap();
        assert_eq!(found.repo_owner.unwrap(), "test");
        assert_eq!(found.repo_name.unwrap(), "repo");
    }

    /// Regression test: `insert_repo` must upsert (overwrite by name) instead
    /// of always appending. This mirrors what `cli_update` does — it
    /// re-inserts the same repo after an update — and previously produced
    /// duplicate entries in the db.
    #[test]
    fn db_insert_repo_upsert_no_duplicates() -> Result<(), Box<dyn std::error::Error>> {
        let (_dir, path) = temp_db_path();
        let db = Db::create_or_open(&path)?;

        let mut repo = Repo::new("abc")
            .by_url("https://github.com/owner/abc")
            .unwrap();
        repo.version = Some("v1.0.0".into());
        db.insert_repo(repo.clone())?;

        // Simulate an update: bump version then re-insert.
        let mut updated = repo.clone();
        updated.version = Some("v2.0.0".into());
        db.insert_repo(updated)?;

        let all = db.get_repo_list();
        assert_eq!(all.len(), 1, "update must not create a duplicate repo");
        let got = db.get_repo("abc").unwrap();
        assert_eq!(got.version.as_deref(), Some("v2.0.0"));

        // A second, different repo must not be affected.
        db.insert_repo(
            Repo::new("xyz")
                .by_url("https://github.com/owner/xyz")
                .unwrap(),
        )?;
        assert_eq!(db.get_repo_list().len(), 2);
        Ok(())
    }
}
