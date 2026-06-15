//! Defines the [`Repo`] structure.

pub mod db;

use std::{
    cmp::Ordering,
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use log::debug;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use crate::utils::{UrlJoinAll, table::Table};

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
#[serde(rename_all = "lowercase")]
pub enum Site {
    #[default]
    Github,
}

impl Site {
    #[must_use]
    pub fn base(&self) -> Url {
        let url = match self {
            Self::Github => "https://github.com",
        };
        Url::parse(url).expect("hardcoded URL should be valid")
    }

    #[must_use]
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

fn null_to_empty_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    // Attempt to deserialize as Option<Vec<T>>
    let opt = Option::<Vec<T>>::deserialize(d)?;
    Ok(opt.unwrap_or_default())
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !(*b)
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(b: &bool) -> bool {
    *b
}

fn default_true() -> bool {
    true
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct Repo {
    pub name: String,
    pub bin_name: String,
    pub site: Site,
    pub repo_name: Option<String>,
    pub repo_owner: Option<String>,
    pub asset: Option<String>,
    pub version: Option<String>,
    pub installed_files: Vec<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_time: Option<std::time::SystemTime>,

    /// Prefer musl builds over gnu when selecting assets (default: prefer gnu)
    #[serde(default, skip_serializing_if = "is_false")]
    pub prefer_musl: bool,

    /// Backward compatibility: old db may have `prefer_gnu` field.
    /// `prefer_gnu: true` meant prefer gnu (now the default, so no-op).
    /// Read during deserialization but never serialized.
    #[serde(default, skip_serializing)]
    pub prefer_gnu: bool,

    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub no_pre: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub one_bin: bool,

    #[serde(
        default,
        deserialize_with = "null_to_empty_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub asset_filter: Vec<String>,

    #[cfg(windows)]
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_msi: bool,

    /// Whether the package was installed with --interactive
    #[serde(default, skip_serializing_if = "is_false")]
    pub interactive: bool,

    /// Whether the package was installed from a local path
    #[serde(default, skip_serializing_if = "is_false")]
    pub local: bool,
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
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        #[cfg(windows)]
        assert!(
            !["app", "bin"].contains(&name.as_str()),
            "Invalid repo name: `{name}`. Must not be `app` or `bin`"
        );
        Self {
            name: name.clone(),
            ..Default::default()
        }
        .with_bin_name(name)
    }

    #[must_use]
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

    #[must_use]
    pub fn url(&self) -> Option<Url> {
        let owner = self.repo_owner.as_deref()?;
        let repo_name = self.repo_name.as_deref()?;
        self.site.base().join_all_str([owner, repo_name]).ok()
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

    pub fn set_by_fullname(&mut self, full_name: &str) -> Result<()> {
        let res = split_full_name(full_name)?;
        debug!("set repo_name: {}, repo_owner: {}", res.1, res.0);
        self.repo_name = Some(res.1);
        self.repo_owner = Some(res.0);
        Ok(())
    }

    pub fn set_by_url(&mut self, url: &str) -> Result<()> {
        let parsed = Url::parse(url).map_err(|e| anyhow!("Invalid URL '{url}': {e}"))?;
        self.set_by_fullname(parsed.path())
    }

    pub fn by_url(mut self, url: &str) -> Result<Self> {
        self.set_by_url(url)?;
        Ok(self)
    }

    pub fn by_fullname(mut self, full_name: &str) -> Result<Self> {
        self.set_by_fullname(full_name)?;
        Ok(self)
    }
}

impl From<Url> for Repo {
    #[inline]
    fn from(value: Url) -> Self {
        if let Ok((owner, name)) = split_full_name(value.path()) {
            let mut repo = Self::new(name.clone());
            repo.repo_name = Some(name);
            repo.repo_owner = Some(owner);
            repo
        } else {
            // Fallback: use the last non-empty path segment as name
            let name = value
                .path_segments()
                .and_then(|mut s| s.next_back())
                .filter(|n| !n.is_empty())
                .unwrap_or("unknown");
            Self::new(name)
        }
    }
}

impl From<&str> for Repo {
    #[inline]
    fn from(value: &str) -> Self {
        match Url::parse(value) {
            Ok(url) => Self::from(url),
            Err(_) => Self::new(value),
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
)]
pub struct RepoList(Vec<Repo>);

impl RepoList {
    #[must_use]
    pub fn new(repos: Vec<Repo>) -> Self {
        Self(repos)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Repo] {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> Vec<Repo> {
        self.0
    }

    pub fn push(&mut self, repo: Repo) {
        self.0.push(repo);
    }

    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&Repo) -> bool,
    {
        self.0.retain(f);
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Repo> {
        self.0.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a> IntoIterator for &'a RepoList {
    type Item = &'a Repo;
    type IntoIter = std::slice::Iter<'a, Repo>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl From<Vec<Repo>> for RepoList {
    fn from(v: Vec<Repo>) -> Self {
        Self(v)
    }
}

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
        let repo = Repo::new("abc")
            .by_url("https://github.com/lxl66566/bpm-rs/")
            .unwrap();
        assert_eq!(
            repo.url().unwrap().as_str(),
            "https://github.com/lxl66566/bpm-rs"
        );
        assert_eq!(repo.repo_name.unwrap(), "bpm-rs");
        assert_eq!(repo.repo_owner.unwrap(), "lxl66566");
    }

    #[test]
    fn test_from_str_url() {
        let repo = Repo::from("https://github.com/owner/repo");
        assert_eq!(repo.name, "repo");
        assert_eq!(repo.repo_owner.unwrap(), "owner");
    }

    #[test]
    fn test_from_str_name() {
        let repo = Repo::from("my-package");
        assert_eq!(repo.name, "my-package");
        assert!(repo.repo_owner.is_none());
    }

    #[test]
    fn test_with_bin_name() {
        let repo = Repo::new("test").with_bin_name("mybin".to_string());
        #[cfg(windows)]
        assert_eq!(repo.bin_name, "mybin.exe");
        #[cfg(not(windows))]
        assert_eq!(repo.bin_name, "mybin");

        let repo = Repo::new("test").with_bin_name("mybin.exe".to_string());
        assert_eq!(repo.bin_name, "mybin.exe");
    }

    #[test]
    fn test_repo_list_display() {
        let list = RepoList::new(vec![
            Repo::new("bpm-rs")
                .by_url("https://github.com/lxl66566/bpm-rs")
                .unwrap(),
        ]);
        let s = format!("{list}");
        println!("{s}");
    }
}
