use std::path::PathBuf;

use bpm::storage::{
    Repo,
    db::{Db, DbOperation},
};

fn temp_db_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_db.ron");
    (dir, path)
}

#[test]
fn db_insert_and_query() {
    let (_dir, path) = temp_db_path();
    let db = Db::create_or_open(&path).unwrap();

    let repo = Repo::new("bpm").by_url("https://github.com/lxl66566/bpm-rs");
    db.insert_repo(repo.clone()).unwrap();

    let found = db.get_repo("bpm").unwrap();
    assert_eq!(found.name, "bpm");
    assert_eq!(found.repo_owner.unwrap(), "lxl66566");

    assert!(db.get_repo("nonexistent").is_none());
}

#[test]
fn db_insert_multiple_and_list() {
    let (_dir, path) = temp_db_path();
    let db = Db::create_or_open(&path).unwrap();

    let repos = vec![
        Repo::new("delta").by_url("https://github.com/dandavison/delta"),
        Repo::new("bat").by_url("https://github.com/sharkdp/bat"),
        Repo::new("eza").by_url("https://github.com/eza-community/eza"),
    ];
    for repo in &repos {
        db.insert_repo(repo.clone()).unwrap();
    }

    let list = db.get_repo_list();
    assert_eq!(list.len(), 3);

    assert!(db.get_repo("bat").is_some());
    assert!(db.get_repo("delta").is_some());
    assert!(db.get_repo("eza").is_some());
}

#[test]
fn db_remove() {
    let (_dir, path) = temp_db_path();
    let db = Db::create_or_open(&path).unwrap();

    db.insert_repo(Repo::new("alpha").by_url("https://github.com/a/alpha"))
        .unwrap();
    db.insert_repo(Repo::new("beta").by_url("https://github.com/b/beta"))
        .unwrap();

    assert_eq!(db.get_repo_list().len(), 2);

    db.remove_repo("alpha").unwrap();
    assert!(db.get_repo("alpha").is_none());
    assert!(db.get_repo("beta").is_some());
    assert_eq!(db.get_repo_list().len(), 1);
}

#[test]
fn db_persistence() {
    let (_dir, path) = temp_db_path();

    {
        let db = Db::create_or_open(&path).unwrap();
        db.insert_repo(Repo::new("persist-test").by_url("https://github.com/test/repo"))
            .unwrap();
    }

    let db2 = Db::create_or_open(&path).unwrap();
    let found = db2.get_repo("persist-test").unwrap();
    assert_eq!(found.repo_owner.unwrap(), "test");
    assert_eq!(found.repo_name.unwrap(), "repo");
}

#[test]
fn db_remove_nonexistent() {
    let (_dir, path) = temp_db_path();
    let db = Db::create_or_open(&path).unwrap();

    db.insert_repo(Repo::new("only-one").by_url("https://github.com/a/b"))
        .unwrap();

    let result = db.remove_repo("ghost");
    assert!(result.is_ok());
    assert_eq!(db.get_repo_list().len(), 1);
}
