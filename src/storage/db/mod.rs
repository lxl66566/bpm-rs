pub mod file;

use std::path::Path;

use anyhow::Result;
pub use file::Db;

use super::{Repo, RepoList};

pub trait DbOperation {
    fn create_or_open(path: impl AsRef<Path>) -> Result<Self>
    where
        Self: Sized;
    fn get_repo_list(&self) -> RepoList;
    fn get_repo(&self, name: &str) -> Option<Repo>;
    fn insert_repo(&self, repo: Repo) -> Result<()>;
    fn remove_repo(&self, name: &str) -> Result<()>;
}
