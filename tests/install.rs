use std::{fs, path::PathBuf};

use bpm::{
    context::Context,
    installation::{Installation, only_one_file_in_dir},
    storage::{Repo, db::DbOperation},
};

fn test_ctx() -> (tempfile::TempDir, Context) {
    let dir = tempfile::tempdir().unwrap();
    let install_pos = dir.path().join("bpm");
    let db_path = dir.path().join("db");
    let ctx = Context::new()
        .with_install_position(&install_pos)
        .with_db_path(&db_path);
    (dir, ctx)
}

fn test_ctx_with_dry_run() -> Context {
    let dir = tempfile::tempdir().unwrap();
    Context::new()
        .with_dry_run(true)
        .with_install_position(dir.path().join("bpm"))
        .with_db_path(tempfile::tempdir().unwrap().path().join("db"))
}

fn create_fake_binary_tree(dir: &std::path::Path, bin_name: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(bin_name), "#!/bin/sh\necho hello").unwrap();
}

fn create_full_package_tree(dir: &std::path::Path, bin_name: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(bin_name), "binary-content").unwrap();

    let lib_dir = dir.join("lib");
    fs::create_dir_all(&lib_dir).unwrap();
    fs::write(lib_dir.join("libfoo.a"), "lib-content").unwrap();

    let share_dir = dir.join("share");
    fs::create_dir_all(&share_dir).unwrap();
    fs::write(share_dir.join("data.txt"), "share-content").unwrap();
}

#[cfg(windows)]
#[test]
fn windows_install_moves_files() {
    let (_guard, ctx) = test_ctx();
    let src_dir = tempfile::tempdir().unwrap();

    fs::create_dir_all(src_dir.path().join("sub")).unwrap();
    fs::write(src_dir.path().join("app.exe"), "binary").unwrap();
    fs::write(src_dir.path().join("sub").join("config.toml"), "config").unwrap();

    let mut repo = Repo::new("test-app");
    repo.bin_name = "app.exe".to_string();

    repo.install(src_dir.path(), &ctx).unwrap();

    let app_dir = ctx.app_path().join("test-app");
    assert!(app_dir.exists());
    assert!(app_dir.join("app.exe").exists());
    assert!(app_dir.join("sub").join("config.toml").exists());
}

#[cfg(windows)]
#[test]
fn windows_uninstall_removes_files() {
    let (_guard, ctx) = test_ctx();
    let src_dir = tempfile::tempdir().unwrap();

    fs::write(src_dir.path().join("hello.exe"), "binary").unwrap();

    let mut repo = Repo::new("hello");
    repo.bin_name = "hello.exe".to_string();

    repo.install(src_dir.path(), &ctx).unwrap();
    assert!(ctx.app_path().join("hello").exists());

    repo.uninstall(&ctx).unwrap();
    assert!(!ctx.app_path().join("hello").exists());
}

#[test]
fn dry_run_does_not_modify_filesystem() {
    let ctx = test_ctx_with_dry_run();

    let src_dir = tempfile::tempdir().unwrap();
    fs::write(src_dir.path().join("hello.txt"), "content").unwrap();

    let mut repo = Repo::new("dry-test");
    repo.bin_name = "hello.txt".to_string();

    let result = repo.install(src_dir.path(), &ctx);
    assert!(result.is_ok());

    assert!(
        src_dir.path().join("hello.txt").exists(),
        "dry run should not move source files"
    );
}

#[test]
fn dry_run_uninstall_is_noop() {
    let ctx = test_ctx_with_dry_run();

    let mut repo = Repo::new("noop-test");
    repo.installed_files
        .push(PathBuf::from("/tmp/noop/fake.txt"));

    let result = repo.uninstall(&ctx);
    assert!(result.is_ok());
}

#[test]
fn only_one_file_single_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("only.txt"), "content").unwrap();
    let result = only_one_file_in_dir(dir.path()).unwrap();
    assert_eq!(result, Some(dir.path().join("only.txt")));
}

#[test]
fn only_one_file_single_dir() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("subdir")).unwrap();
    let result = only_one_file_in_dir(dir.path()).unwrap();
    assert_eq!(result, Some(dir.path().join("subdir")));
}

#[test]
fn only_one_file_multiple() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    let result = only_one_file_in_dir(dir.path()).unwrap();
    assert!(result.is_none());
}

#[test]
fn only_one_file_empty() {
    let dir = tempfile::tempdir().unwrap();
    let result = only_one_file_in_dir(dir.path()).unwrap();
    assert!(result.is_none());
}

#[test]
fn install_and_uninstall_roundtrip_db() {
    let base = tempfile::tempdir().unwrap();
    let ctx = Context::new()
        .with_install_position(base.path().join("bpm"))
        .with_db_path(base.path().join("db.ron"));

    let db = ctx.db().unwrap();
    let mut repo = Repo::new("roundtrip");
    repo.bin_name = "roundtrip.txt".to_string();
    repo.repo_owner = Some("test".to_string());
    repo.repo_name = Some("roundtrip".to_string());
    repo.version = Some("1.0.0".to_string());

    db.insert_repo(repo.clone()).unwrap();

    let found = db.get_repo("roundtrip").unwrap();
    assert_eq!(found.version.unwrap(), "1.0.0");

    db.remove_repo("roundtrip").unwrap();
    assert!(db.get_repo("roundtrip").is_none());
}
