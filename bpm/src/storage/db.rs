use std::path::Path;

use anyhow::anyhow;
use native_db::{Builder, Database, Models};

use super::{Repo, RepoList};

type Result<T, E = native_db::db_type::Error> = std::result::Result<T, E>;

pub trait DbInit {
    fn create_or_open<'a>(
        &self,
        models: &'a Models,
        path: impl AsRef<Path>,
    ) -> Result<Database<'a>>;
}

impl DbInit for Builder {
    fn create_or_open<'a>(
        &self,
        models: &'a Models,
        path: impl AsRef<Path>,
    ) -> Result<Database<'a>> {
        if path.as_ref().exists() {
            self.open(models, path)
        } else {
            self.create(models, path)
        }
    }
}

pub trait DbOperation {
    fn get_repo_list(&self) -> Result<RepoList>;
    fn get_repo(&self, name: &str) -> Result<Option<Repo>>;
    fn insert_repo(&self, repo: Repo) -> Result<()>;
    fn remove_repo(&self, name: &str) -> anyhow::Result<()>;
}

impl DbOperation for Database<'_> {
    fn get_repo_list(&self) -> Result<RepoList> {
        Ok(self
            .r_transaction()?
            .scan()
            .primary()
            .expect("failed to scan people")
            .all()
            .collect::<Result<Vec<_>>>()?
            .into())
    }

    fn get_repo(&self, name: &str) -> Result<Option<Repo>> {
        self.r_transaction()?.get().primary(name)
    }

    fn insert_repo(&self, repo: Repo) -> Result<()> {
        let rw = self.rw_transaction()?;
        rw.insert(repo)?;
        rw.commit()?;
        Ok(())
    }

    fn remove_repo(&self, name: &str) -> anyhow::Result<()> {
        let repo = self
            .get_repo(name)?
            .ok_or_else(|| anyhow!("repo not found"))?;
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
