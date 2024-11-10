pub mod file;

use super::{Repo, RepoList};
use anyhow::Result;
pub use file::Db;
use std::path::Path;

pub trait DbOperation {
    fn create_or_open(path: impl AsRef<Path>) -> Result<Self>
    where
        Self: Sized;
    fn get_repo_list(&self) -> RepoList;
    fn get_repo(&self, name: &str) -> Option<Repo>;
    fn insert_repo(&self, repo: Repo) -> Result<()>;
    fn remove_repo(&self, name: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use file::Db;

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
