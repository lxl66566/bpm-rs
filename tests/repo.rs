use std::fs;

use bpm::{
    storage::{Repo, RepoList},
    utils::{
        path::PathExt,
        url::{UrlJoinAll, UrlOps},
    },
};
use url::Url;

#[test]
fn repo_from_url_parses_correctly() {
    let repo = Repo::from("https://github.com/owner/repo");
    assert_eq!(repo.name, "repo");
    assert_eq!(repo.repo_owner.as_deref(), Some("owner"));
    assert_eq!(repo.repo_name.as_deref(), Some("repo"));
    assert_eq!(
        repo.url().unwrap().as_str(),
        "https://github.com/owner/repo"
    );
}

#[test]
fn repo_from_url_with_trailing_slash() {
    let repo = Repo::from("https://github.com/lxl66566/bpm-rs/");
    assert_eq!(repo.repo_name.unwrap(), "bpm-rs");
    assert_eq!(repo.repo_owner.unwrap(), "lxl66566");
}

#[test]
fn repo_from_name() {
    let repo = Repo::from("my-package");
    assert_eq!(repo.name, "my-package");
    assert!(repo.repo_owner.is_none());
    assert!(repo.url().is_none());
}

#[test]
fn repo_with_bin_name() {
    let repo = Repo::new("test").with_bin_name("custom-bin".to_string());
    #[cfg(windows)]
    assert_eq!(repo.bin_name, "custom-bin.exe");
    #[cfg(not(windows))]
    assert_eq!(repo.bin_name, "custom-bin");
}

#[test]
fn repo_by_fullname() {
    let repo = Repo::new("x").by_fullname("owner/package");
    assert_eq!(repo.repo_owner.unwrap(), "owner");
    assert_eq!(repo.repo_name.unwrap(), "package");
}

#[test]
fn repo_file_list_dedup() {
    let mut repo = Repo::new("test");
    repo.add_file_list("/a/b");
    repo.add_file_list("/c/d");
    repo.add_file_list("/a/b");
    repo.add_file_list("/e/f");
    assert_eq!(repo.installed_files.len(), 4);

    repo.dedup_file_list();
    assert_eq!(repo.installed_files.len(), 3);
}

#[test]
fn repo_list_display_not_empty() {
    let list = RepoList(vec![
        Repo::new("bpm-rs").by_url("https://github.com/lxl66566/bpm-rs"),
        Repo::new("delta").by_url("https://github.com/dandavison/delta"),
    ]);
    let s = format!("{list}");
    assert!(!s.is_empty());
    assert!(s.contains("bpm-rs"));
    assert!(s.contains("delta"));
}

#[test]
fn repo_list_empty_display() {
    let list = RepoList(vec![]);
    let s = format!("{list}");
    assert!(!s.is_empty());
}

#[test]
fn repo_site_urls() {
    let repo = Repo::new("test");
    let base = repo.site.base();
    assert!(base.as_str().starts_with("https://github.com"));
    let api = repo.site.api_base();
    assert!(api.as_str().starts_with("https://api.github.com"));
}

#[test]
fn url_join_all() {
    let base = Url::parse("https://api.github.com").unwrap();
    let result = base
        .join_all_str(["repos", "owner", "repo", "releases", "latest"])
        .unwrap();
    assert_eq!(
        result.as_str(),
        "https://api.github.com/repos/owner/repo/releases/latest"
    );
}

#[test]
fn url_extension() {
    let url = Url::parse("https://example.com/file.tar.gz").unwrap();
    assert_eq!(url.extension(), Some("gz"));

    let url_no_ext = Url::parse("https://example.com/path/").unwrap();
    assert_eq!(url_no_ext.extension(), None);
}

#[test]
fn path_ext_create_and_remove() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("nested").join("deep").join("dir");

    target.create_dir_if_not_exist().unwrap();
    assert!(target.exists());
    assert!(target.is_dir());

    target.create_dir_if_not_exist().unwrap();
    target.create_dir_if_not_exist().unwrap();

    target.remove_all_allow_missing().unwrap();
    assert!(!target.exists());

    target.remove_all_allow_missing().unwrap();
    target.remove_all_allow_missing().unwrap();
}

#[test]
fn path_ext_is_subpath_of() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("a").join("b");

    assert!(child.is_subpath_of(dir.path()));
    assert!(!dir.path().is_subpath_of(&child));
    assert!(dir.path().is_subpath_of(dir.path()));
}

#[test]
fn path_ext_glob_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("target.txt"), "content").unwrap();
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("target.txt"), "content2").unwrap();
    fs::write(dir.path().join("other.txt"), "content3").unwrap();

    let results = dir.path().glob_name("target.txt");
    assert_eq!(results.len(), 2);
}

#[test]
fn repo_ordering() {
    let mut repos = [Repo::new("charlie"), Repo::new("alpha"), Repo::new("bravo")];
    repos.sort();
    assert_eq!(repos[0].name, "alpha");
    assert_eq!(repos[1].name, "bravo");
    assert_eq!(repos[2].name, "charlie");
}

#[test]
fn repo_serialization_roundtrip() {
    let mut repo = Repo::new("serde-test");
    repo.repo_owner = Some("owner".to_string());
    repo.repo_name = Some("repo".to_string());
    repo.version = Some("v1.2.3".to_string());
    repo.asset = Some("https://example.com/file.tar.gz".to_string());
    repo.installed_files.push("/usr/bin/serde-test".into());
    repo.prefer_gnu = true;

    let json = serde_json::to_string(&repo).unwrap();
    let deserialized: Repo = serde_json::from_str(&json).unwrap();

    assert_eq!(repo.name, deserialized.name);
    assert_eq!(repo.repo_owner, deserialized.repo_owner);
    assert_eq!(repo.version, deserialized.version);
    assert_eq!(repo.asset, deserialized.asset);
    assert_eq!(repo.installed_files, deserialized.installed_files);
    assert_eq!(repo.prefer_gnu, deserialized.prefer_gnu);
}
