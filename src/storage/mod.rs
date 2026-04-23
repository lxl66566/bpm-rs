//! Defines the [`Repo`] structure.

pub mod db;

use crate::utils::{UrlJoinAll, table::Table};
use anyhow::{Result, anyhow};
use assert2::assert;
use die_exit::DieWith;
use log::debug;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    fmt,
    path::{Path, PathBuf},
};
use tap::Tap;
use url::Url;

/// Split a full name into the first and second part.
///
/// Returns error if the full name contains not exactly 2 parts.
///
/// # Example
///
/// With the full name `me/myrepo`, the `repo_owner` would be `me`, and the
/// `repo_name` would be `myrepo`.
fn split_full_name(full_name: &str) -> Result<(String, String)> {
    let mut iter = full_name
        .trim_matches(|x: char| x == '/' || x.is_ascii_whitespace())
        .split('/');
    let res = (
        iter.next()
            .ok_or_else(|| anyhow!("1st part of full name is empty"))?
            .to_string(),
        iter.next()
            .ok_or_else(|| anyhow!("2nd part of full name is empty"))?
            .to_string(),
    );
    debug_assert!(iter.count() == 0, "fullname has more than 2 parts");

    Ok(res)
}

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
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Repo {
    pub name: String,
    pub bin_name: String,
    pub site: Site,
    pub repo_name: Option<String>,
    pub repo_owner: Option<String>,
    pub asset: Option<String>,
    pub version: Option<String>,
    pub installed_files: Vec<PathBuf>,
    pub installed_time: Option<std::time::SystemTime>,
    pub prefer_gnu: bool,
    pub no_pre: bool,
    pub one_bin: bool,
    #[cfg(windows)]
    pub is_msi: bool,
}

impl Default for Repo {
    fn default() -> Self {
        Self {
            name: String::new(),
            #[cfg(not(windows))]
            bin_name: "".into(),
            #[cfg(windows)]
            bin_name: "*.exe".into(),
            site: Site::default(),
            repo_name: None,
            repo_owner: None,
            asset: None,
            version: None,
            installed_files: Vec::new(),
            installed_time: None,
            prefer_gnu: false,
            no_pre: false,
            one_bin: false,
            #[cfg(windows)]
            is_msi: false,
        }
    }
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
            ..Default::default()
        }
    }

    pub fn with_bin_name(mut self, bin_name: String) -> Self {
        #[cfg(windows)]
        {
            self.bin_name = if std::path::Path::new(&bin_name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
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

    pub fn url(&self) -> Option<Url> {
        Some(
            self.site
                .base()
                .join_all_str([self.repo_owner.as_deref()?, self.repo_name.as_deref()?])
                .die_with(|e| format!("trying to construct an invalid url. Err: {e}")),
        )
    }

    pub fn dedup_file_list(&mut self) {
        self.installed_files.sort();
        self.installed_files.dedup();
        debug!("dedup file list success: {:#?}", self.installed_files);
    }

    pub fn add_file_list(&mut self, file: impl AsRef<Path>) {
        let file = file.as_ref().to_path_buf();
        debug!("add file `{}` to file_list", file.display());
        self.installed_files.push(file);
    }

    /// Set the `repo_name` and `repo_owner` by fullname.
    ///
    /// # Example
    ///
    /// With the full name `me/myrepo`, the `repo_owner` would be `me`, and the
    /// `repo_name` would be `myrepo`.
    ///
    /// # Errors
    ///
    /// Returns error if the full name contains not exactly 2 parts.
    pub fn set_by_fullname(&mut self, full_name: &str) -> Result<()> {
        let res = split_full_name(full_name)?;
        debug!("set repo_name: {}, repo_owner: {}", res.1, res.0);
        self.repo_name = Some(res.1);
        self.repo_owner = Some(res.0);
        Ok(())
    }

    /// Set the `repo_name` and `repo_owner` by url.
    ///
    /// # Example
    ///
    /// with the url `https://github.com/lxl66566/bpm-rs/`, the `repo_owner` would be `lxl66566`, and the `repo_name` would be `bpm-rs`.
    pub fn set_by_url(&mut self, url: &str) {
        let binding = Url::parse(url).expect("parsing invalid URL.");
        let full_name = binding.path();
        self.set_by_fullname(full_name).unwrap();
    }

    pub fn by_url(mut self, url: &str) -> Self {
        self.set_by_url(url);
        self
    }

    pub fn by_fullname(mut self, full_name: &str) -> Self {
        self.set_by_fullname(full_name).unwrap();
        self
    }
}

impl From<Url> for Repo {
    /// Construct a repo from a url.
    ///
    /// # Panics
    ///
    /// Panics if the url is invalid.
    #[inline]
    fn from(value: Url) -> Self {
        let fullname = value.path();
        let res = split_full_name(fullname).expect("construct repo from invalid URL.");
        Self::default().tap_mut(|repo| {
            repo.name = res.1.clone();
            repo.repo_name = Some(res.1);
            repo.repo_owner = Some(res.0);
        })
    }
}

impl From<&str> for Repo {
    /// Construct a repo from a string, could be a url or a name.
    #[inline]
    fn from(value: &str) -> Self {
        let name = value;
        let url = Url::parse(name);
        if let Ok(url) = url {
            Self::from(url)
        } else {
            Self::new(name)
        }
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

pub struct RepoList(pub Vec<Repo>);

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
        assert_eq!(
            repo.url().unwrap().as_str(),
            "https://github.com/lxl66566/bpm-rs"
        );
        assert_eq!(repo.repo_name.unwrap(), "bpm-rs");
        assert_eq!(repo.repo_owner.unwrap(), "lxl66566");
    }
}
