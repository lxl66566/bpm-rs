pub mod db;

use std::{cmp::Ordering, fmt, path::PathBuf, sync::LazyLock as Lazy};

use die_exit::{die, DieWith};
use log::debug;
use native_db::{native_db, Models, ToKey};
use native_model::{native_model, Model};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::utils::{table::Table, UrlJoinAll};

pub static MODELS: Lazy<Models> = Lazy::new(|| {
    let mut models = Models::new();
    models.define::<Repo>().unwrap();
    models
});

#[non_exhaustive]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub enum Site {
    #[default]
    Github,
}

impl Site {
    pub fn base(&self) -> Url {
        let url = match self {
            Self::Github => "https://github.com",
        };
        Url::parse(url).expect("hardcoded URL should be valid")
    }

    pub fn api_base(&self) -> Url {
        let url = match self {
            Self::Github => "https://api.github.com",
        };
        Url::parse(url).expect("hardcoded URL should be valid")
    }
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Github => write!(f, "github"),
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[native_model(id = 1, version = 1)]
#[native_db]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Repo {
    #[primary_key]
    pub name: String,
    pub bin_name: String,
    pub site: Site,
    pub repo_name: Option<String>,
    pub repo_owner: Option<String>,
    pub asset: Option<String>,
    pub version: Option<String>,
    pub installed_files: Vec<PathBuf>,
    pub prefer_gnu: bool,
    pub no_pre: bool,
    pub one_bin: bool,
    #[cfg(windows)]
    pub is_msi: bool,
}

impl Ord for Repo {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialOrd for Repo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Repo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = Table::default().with_repo(self);
        write!(f, "{table}")
    }
}

impl Repo {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        #[cfg(windows)]
        assert!(
            !["app", "bin"].contains(&name.as_str()),
            "Invalid repo name: `{name}`. Must not be one of them: `app`, `bin`"
        );
        Self {
            #[cfg(not(windows))]
            name: name.clone(),
            #[cfg(windows)]
            name,
            #[cfg(not(windows))]
            bin_name: name,
            #[cfg(windows)]
            bin_name: "*.exe".into(),
            site: Site::default(),
            repo_name: None,
            repo_owner: None,
            asset: None,
            version: None,
            installed_files: Vec::new(),
            prefer_gnu: false,
            no_pre: false,
            one_bin: false,
            #[cfg(windows)]
            is_msi: false,
        }
    }

    pub fn with_bin_name(mut self, bin_name: String) -> Self {
        #[cfg(windows)]
        {
            self.bin_name = if std::path::Path::new(&bin_name)
                .extension()
                .map_or(false, |ext| ext.eq_ignore_ascii_case("exe"))
            {
                bin_name
            } else {
                bin_name + ".exe"
            };
        }
        #[cfg(not(windows))]
        {
            self.bin_name = bin_name;
        }
        self
    }

    pub fn url(&self) -> Url {
        assert!(
            self.repo_name.is_some() || self.repo_owner.is_some(),
            "repo_name and repo_owner must be set"
        );
        self.site
            .base()
            .join_all_str([
                self.repo_owner.as_deref().unwrap(),
                self.repo_name.as_deref().unwrap(),
            ])
            .die_with(|e| format!("trying to construct an invalid url. Err: {e}"))
    }

    pub fn dedup_file_list(&mut self) {
        self.installed_files.sort();
        self.installed_files.dedup();
        debug!("dedup file list success: {:#?}", self.installed_files);
    }

    pub fn add_file_list(&mut self, file: PathBuf) {
        self.installed_files.push(file.clone());
        debug!("added file `{}` to file_list", file.display());
    }

    /// Set the `repo_name` and `repo_owner` by fullname.
    ///
    /// # Example
    ///
    /// With the full name `me/myrepo`, the `repo_owner` would be `me`, and the
    /// `repo_name` would be `myrepo`.
    pub fn set_by_fullname(&mut self, full_name: &str) {
        let mut iter = full_name.trim_matches('/').split('/');
        self.repo_owner = Some(
            iter.next()
                .unwrap_or_else(|| die!("An error occurs in parsing full name 1st part"))
                .to_string(),
        );
        self.repo_name = Some(
            iter.next()
                .unwrap_or_else(|| die!("An error occurs in parsing full name 2nd part"))
                .to_string(),
        );
        debug_assert!(iter.count() == 0, "fullname has more than 2 parts");
        debug!(
            "set repo_name: {}, repo_owner: {}",
            self.repo_name.as_ref().unwrap(),
            self.repo_owner.as_ref().unwrap()
        );
    }

    /// Set the `repo_name` and `repo_owner` by url.
    /// # Example
    ///
    /// with the url `https://github.com/lxl66566/bpm-rs/`, the `repo_owner` would be `lxl66566`, and the `repo_name` would be `bpm-rs`.
    pub fn set_by_url(&mut self, url: &str) {
        let binding = Url::parse(url).expect("parsing invalid URL.");
        let full_name = binding.path();
        self.set_by_fullname(full_name);
    }

    pub fn by_url(mut self, url: &str) -> Self {
        self.set_by_url(url);
        self
    }

    pub fn by_fullname(mut self, full_name: &str) -> Self {
        self.set_by_fullname(full_name);
        self
    }
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Default,
    derive_more::From,
    derive_more::Deref,
    derive_more::DerefMut,
)]
pub struct RepoList(Vec<Repo>);

impl fmt::Display for RepoList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut table = Table::default();
        for repo in &self.0 {
            table.add_row(repo);
        }
        write!(f, "{table}")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_set_by_url() {
        let repo = Repo::new("abc").by_url("https://github.com/lxl66566/bpm-rs/");
        assert_eq!(repo.url().as_str(), "https://github.com/lxl66566/bpm-rs");
        assert_eq!(repo.repo_name.unwrap(), "bpm-rs");
        assert_eq!(repo.repo_owner.unwrap(), "lxl66566");
    }
}
