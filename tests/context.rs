use bpm::{context::Context, storage::db::DbOperation};

#[test]
fn context_default_paths() {
    let ctx = Context::new();
    assert!(ctx.app_path().to_str().unwrap().contains("app"));
    assert!(ctx.bin_path().to_str().unwrap().contains("bin"));
    assert!(!ctx.dry_run);
    assert!(!ctx.quiet);
}

#[test]
fn context_builder() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = Context::new()
        .with_dry_run(true)
        .with_quiet(true)
        .with_install_position(tmp.path().join("custom_install"))
        .with_db_path(tmp.path().join("custom_db"));

    assert!(ctx.dry_run);
    assert!(ctx.quiet);
    assert_eq!(
        ctx.app_path(),
        tmp.path().join("custom_install").join("app")
    );
    assert_eq!(
        ctx.bin_path(),
        tmp.path().join("custom_install").join("bin")
    );
}

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
