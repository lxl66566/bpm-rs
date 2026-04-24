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
