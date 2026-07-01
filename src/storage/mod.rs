//! Defines the [`Repo`] structure.

pub mod db;

use std::{
    cmp::Ordering,
    collections::BTreeSet,
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

/// Preference for libc variant when selecting assets.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibcPref {
    #[default]
    Gnu,
    Musl,
}

impl LibcPref {
    /// Returns the keyword used in asset filenames.
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Gnu => "gnu",
            Self::Musl => "musl",
        }
    }

    #[must_use]
    pub fn is_gnu(&self) -> bool {
        matches!(self, Self::Gnu)
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

/// The tracked set of paths installed by a repo.
///
/// A `BTreeSet` (rather than a `Vec`) so duplicates are impossible without a
/// manual dedup step, and iteration is deterministically sorted — reverse
/// iteration therefore removes children before their parent directories.
pub type InstalledFiles = BTreeSet<PathBuf>;

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
    /// Files tracked as installed by this repo.
    ///
    /// Stored as a `BTreeSet` so paths are naturally de-duplicated (an update
    /// re-runs `install` on a repo that already carries the same paths) and
    /// iterated in a deterministic, sorted order — which also means a reverse
    /// iteration removes children before their parent directories.
    #[serde(default = "InstalledFiles::new")]
    pub installed_files: InstalledFiles,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_time: Option<std::time::SystemTime>,

    /// Preference for libc variant (default: gnu)
    #[serde(default, skip_serializing_if = "LibcPref::is_gnu")]
    pub libc_pref: LibcPref,

    /// Include pre-release versions when selecting assets
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_pre: bool,

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

    pub fn add_file_list(&mut self, file: impl AsRef<Path>) {
        let file = file.as_ref().to_path_buf();
        debug!("add file `{}` to file_list", file.display());
        self.installed_files.insert(file);
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
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

    /// Insert or replace a repo matched by name (upsert).
    ///
    /// If a repo with the same name already exists it is replaced in place,
    /// otherwise the new repo is appended. This keeps the list free of
    /// duplicates and is what makes `update` overwrite the previous record
    /// instead of creating a new one.
    pub fn upsert(&mut self, repo: Repo) {
        if let Some(existing) = self.0.iter_mut().find(|x| x.name == repo.name) {
            *existing = repo;
        } else {
            self.0.push(repo);
        }
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
    fn test_installed_files_natural_dedup() {
        // installed_files is a BTreeSet: inserting the same path twice (which
        // happens when an update re-runs install) must not create duplicates.
        let mut repo = Repo::new("foo");
        let p1: PathBuf = "/usr/bin/foo".into();
        let p2: PathBuf = "/usr/share/foo".into();

        for _ in 0..2 {
            repo.add_file_list(&p1);
            repo.add_file_list(&p2);
        }

        assert_eq!(repo.installed_files.len(), 2);
        assert!(repo.installed_files.contains(&p1));
        assert!(repo.installed_files.contains(&p2));
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

    #[test]
    fn test_upsert_inserts_new_repo() {
        let mut list = RepoList::default();
        list.upsert(Repo::new("foo"));
        list.upsert(Repo::new("bar"));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_upsert_replaces_existing_repo() {
        let mut list = RepoList::default();
        list.upsert(
            Repo::new("foo")
                .by_url("https://github.com/owner/foo")
                .unwrap(),
        );

        // Simulate an update: same name, new version / asset.
        let mut updated = Repo::new("foo")
            .by_url("https://github.com/owner/foo")
            .unwrap();
        updated.version = Some("v2.0.0".into());
        list.upsert(updated);

        // No duplicate should be created.
        assert_eq!(list.len(), 1);
        let repos: Vec<&Repo> = list.iter().collect();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "foo");
        assert_eq!(repos[0].version.as_deref(), Some("v2.0.0"));
    }

    #[test]
    fn test_upsert_preserves_unrelated_repos() {
        let mut list = RepoList::default();
        list.upsert(Repo::new("a"));
        list.upsert(Repo::new("b"));
        list.upsert(Repo::new("c"));

        let mut updated_b = Repo::new("b");
        updated_b.version = Some("v9".into());
        list.upsert(updated_b);

        assert_eq!(list.len(), 3);
        let names: Vec<&str> = list.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(
            list.iter()
                .find(|r| r.name == "b")
                .unwrap()
                .version
                .as_deref(),
            Some("v9")
        );
    }
}
