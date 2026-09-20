mod common;

use std::path::PathBuf;

use bin_package_manager::{
    context::Context,
    installation::{Installation, only_one_file_in_dir},
    storage::{Repo, db::DbOperation},
};
use common::TestEnv;

#[cfg(windows)]
#[test]
fn windows_install_moves_files() {
    let env = TestEnv::new();
    let src_dir = tempfile::tempdir().unwrap();

    std::fs::create_dir_all(src_dir.path().join("sub")).unwrap();
    std::fs::write(src_dir.path().join("app.exe"), "binary").unwrap();
    std::fs::write(src_dir.path().join("sub").join("config.toml"), "config").unwrap();

    let mut repo = Repo::new("test-app");
    repo.bin_name = "app.exe".to_string();

    repo.install(src_dir.path(), &env.ctx()).unwrap();

    let app_dir = env.app_path().join("test-app");
    assert!(app_dir.exists());
    assert!(app_dir.join("app.exe").exists());
    assert!(app_dir.join("sub").join("config.toml").exists());
}

#[cfg(windows)]
#[test]
fn windows_uninstall_removes_files() {
    let env = TestEnv::new();
    let src_dir = tempfile::tempdir().unwrap();

    std::fs::write(src_dir.path().join("hello.exe"), "binary").unwrap();

    let mut repo = Repo::new("hello");
    repo.bin_name = "hello.exe".to_string();

    repo.install(src_dir.path(), &env.ctx()).unwrap();
    assert!(env.app_path().join("hello").exists());

    repo.uninstall(&env.ctx()).unwrap();
    assert!(!env.app_path().join("hello").exists());
}

#[test]
fn dry_run_does_not_modify_filesystem() {
    let env = TestEnv::new();
    let ctx = env.ctx_with_dry_run();

    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("hello.txt"), "content").unwrap();

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
    let env = TestEnv::new();
    let ctx = env.ctx_with_dry_run();

    let mut repo = Repo::new("noop-test");
    repo.installed_files
        .insert(PathBuf::from("/tmp/noop/fake.txt"));

    let result = repo.uninstall(&ctx);
    assert!(result.is_ok());
}

#[test]
fn only_one_file_single_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("only.txt"), "content").unwrap();
    let result = only_one_file_in_dir(dir.path()).unwrap();
    assert_eq!(result, Some(dir.path().join("only.txt")));
}

#[test]
fn only_one_file_single_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("subdir")).unwrap();
    let result = only_one_file_in_dir(dir.path()).unwrap();
    assert_eq!(result, Some(dir.path().join("subdir")));
}

#[test]
fn only_one_file_multiple() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();
    let result = only_one_file_in_dir(dir.path()).unwrap();
    assert!(result.is_none());
}

#[test]
fn only_one_file_empty() {
    let dir = tempfile::tempdir().unwrap();
    let result = only_one_file_in_dir(dir.path()).unwrap();
    assert!(result.is_none());
}

#[cfg(unix)]
#[test]
fn unix_single_binary_install_uninstall_is_idempotent() {
    // Real (non-dry-run) roundtrip. The old code fell through from the
    // single-file branch into the `n == bin_name` arm, installing the same
    // binary twice: the fresh binary was renamed to `mybin.old` and copied
    // again, and uninstall then "revived" it via restore_old.
    let pkg = tempfile::tempdir().unwrap();
    std::fs::write(pkg.path().join("mybin"), "#!/bin/sh\n").unwrap();

    let root = tempfile::tempdir().unwrap();
    // The prefix's bin dir deliberately does NOT pre-exist: install_file
    // must create the destination parents itself (a fresh --prefix or a
    // missing ~/.local/bin used to fail with NotFound).
    let bin_dir = root.path().join("bin");

    let ctx = Context::new().with_prefix(Some(root.path().to_path_buf()));

    let mut repo = Repo::new("mybin");
    repo.bin_name = "mybin".to_string();

    repo.install(pkg.path(), &ctx).unwrap();

    let bin = bin_dir.join("mybin");
    assert!(bin.is_file());
    assert!(!bin_dir.join("mybin.old").exists(), "no .old residue");
    assert_eq!(std::fs::read_dir(&bin_dir).unwrap().count(), 1);

    repo.uninstall(&ctx).unwrap();
    assert!(!bin.exists(), "uninstall must remove the binary");
    assert_eq!(
        std::fs::read_dir(&bin_dir).unwrap().count(),
        0,
        "uninstall must leave nothing behind"
    );
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
