//! use `native_db` as the backend of the storage.

use super::DbOperation;
use crate::storage::{Repo, RepoList, MODELS};
use native_db::Database;
use std::path::Path;

pub type DbType<'a> = Database<'a>;

#[derive(thiserror::Error, Debug)]
pub enum DBError {
    #[error("Database error: {0}")]
    Database(#[from] native_db::db_type::Error),
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Could not find Repo with given name: {0}")]
    KeyNotExist(String),
}

pub fn create_or_open<'a>(path: impl AsRef<Path>) -> Result<DbType<'a>, DBError> {
    let builder = native_db::Builder::new();
    if path.as_ref().exists() {
        builder
            .open(&MODELS, path)
            .map_err(std::convert::Into::into)
    } else {
        builder
            .create(&MODELS, path)
            .map_err(std::convert::Into::into)
    }
}

impl DbOperation for Database<'_> {
    type Result<T> = std::result::Result<T, DBError>;
    fn get_repo_list(&self) -> Self::Result<RepoList> {
        Ok(self
            .r_transaction()?
            .scan()
            .primary()
            .expect("failed to scan people")
            .all()?
            .map(|x| x.map_err(std::convert::Into::into))
            .collect::<Self::Result<Vec<_>>>()?
            .into())
    }

    fn get_repo(&self, name: &str) -> Self::Result<Option<Repo>> {
        Ok(self.r_transaction()?.get().primary(name)?)
    }

    fn insert_repo(&self, repo: Repo) -> Self::Result<()> {
        let rw = self.rw_transaction()?;
        rw.insert(repo)?;
        rw.commit()?;
        Ok(())
    }

    fn remove_repo(&self, name: &str) -> Self::Result<()> {
        let repo = self
            .get_repo(name)?
            .ok_or(DBError::KeyNotExist(name.to_string()))?;
        let rw = self.rw_transaction()?;
        rw.remove(repo)?;
        rw.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use native_db::Builder;

    use super::*;
    use crate::storage::{Repo, MODELS};

    #[test]
    fn test_db_basic_operation() -> Result<(), Box<dyn std::error::Error>> {
        let db = Builder::new().create_in_memory(&MODELS)?;
        let rw = db.rw_transaction()?;
        rw.insert(Repo::new("bpm").by_url("https://github.com/lxl66566/bpm-rs/"))?;
        rw.insert(Repo::new("abd").by_url("https://github.com/lxl6656645/b132/"))?;
        rw.commit()?;
        let all = db.get_repo_list()?;
        assert_eq!(all.0.len(), 2);
        Ok(())
    }
}
