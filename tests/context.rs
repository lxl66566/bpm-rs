use bpm::{context::Context, storage::db::DbOperation};

#[test]
fn context_db_with_custom_path() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = Context::new()
        .with_install_position(tmp.path().join("bpm"))
        .with_db_path(tmp.path().join("my_db.ron"));

    let db = ctx.db().unwrap();
    db.insert_repo(Repo::default()).unwrap(); // need to insert a repo to store db
    assert!(tmp.path().join("my_db.ron").exists());

    use bpm::storage::Repo;
    db.insert_repo(Repo::new("ctx-test").by_url("https://github.com/a/b"))
        .unwrap();
    assert!(db.get_repo("ctx-test").is_some());
}
