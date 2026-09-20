mod common;

use bin_package_manager::{
    cli::{Cli, InstallOptions, SortParam, SubCommand},
    dispatch,
    storage::db::DbOperation,
};
use common::*;

fn install_cli(local_path: &std::path::Path, dry_run: bool) -> Cli {
    Cli {
        command: SubCommand::Install {
            opts: InstallOptions {
                packages: vec!["test-app".to_string()],
                bin_name: None,
                local: Some(local_path.to_path_buf()),
                one_bin: false,
                prefer_musl: false,
                interactive: false,
                filter: vec![],
                name: None,
                pre_release: false,
                sort: SortParam::default(),
            },
            quiet: true,
            dry_run,
        },
        prefix: None,
    }
}

#[tokio::test]
async fn dry_run_install_does_not_persist() {
    let env = TestEnv::new();
    let zip_path = env.tmp().join("test-app.zip");
    create_test_zip(&zip_path, &[("test-app", b"binary")]);

    dispatch(install_cli(&zip_path, true), env.ctx())
        .await
        .unwrap();

    assert!(
        env.db().get_repo("test-app").is_none(),
        "dry run should not create db entry"
    );

    #[cfg(windows)]
    {
        assert!(
            !env.bin_path().join("test-app.cmd").exists(),
            "dry run should not create binary links"
        );
    }
}

/// Windows dry-run must not create the bin dir nor touch the persistent user
/// PATH.
///
/// The install position must be OUTSIDE the temp dir: temp bin paths take the
/// temp-skip inside `ensure_windows_path`, which would hide the real branch.
/// `windows_env::append` mirrors the registry write into the process env, so
/// comparing `PATH` before/after observes the real branch.
#[cfg(windows)]
#[tokio::test]
async fn dry_run_install_has_no_windows_side_effects() {
    use bin_package_manager::context::Context;

    let base = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("dry-run-no-side-effects");
    // Reset from scratch: a previously failed run may have left the dir behind
    // (a panic skips the cleanup at the end).
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let install_pos = base.join("bpm");
    let bin_path = install_pos.join("bin");
    let zip_path = base.join("test-app.zip");
    // The binary name must match the shim scan, otherwise the scan aborts
    // before ever reaching the dry-run guard.
    create_test_zip(&zip_path, &[("test-app.exe", b"binary")]);

    let ctx = Context::new()
        .with_install_position(&install_pos)
        .with_db_path(base.join("db.json"));

    let path_before = std::env::var("PATH").unwrap();
    dispatch(install_cli(&zip_path, true), ctx).await.unwrap();
    let path_after = std::env::var("PATH").unwrap();

    if path_after != path_before {
        // Self-heal so even a regression cannot permanently pollute the
        // machine's PATH, then let the assert below fail.
        let _ = windows_env::remove_from_list("PATH", bin_path.to_string_lossy().as_ref());
    }

    assert_eq!(path_before, path_after, "dry-run must not modify PATH");
    assert!(!bin_path.exists(), "dry-run must not create the bin dir");
    assert!(
        !install_pos.join("app").exists(),
        "dry-run must not create the app dir"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Dry-run from a local directory reads the source in place (no copy to
/// temp) and leaves both source and db untouched.
#[tokio::test]
async fn dry_run_install_from_local_directory() {
    let env = TestEnv::new();
    let src_dir = env.tmp().join("unpacked");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("test-app"), b"binary").unwrap();

    dispatch(install_cli(&src_dir, true), env.ctx())
        .await
        .unwrap();

    assert!(
        src_dir.join("test-app").exists(),
        "dry run must not touch the source directory"
    );
    assert!(
        env.db().get_repo("test-app").is_none(),
        "dry run should not create db entry"
    );
}

#[tokio::test]
async fn dry_run_context_remove_keeps_files() {
    let env = TestEnv::new();
    let zip_path = env.tmp().join("test-app.zip");
    create_test_zip(&zip_path, &[("test-app", b"binary")]);

    dispatch(install_cli(&zip_path, false), env.ctx())
        .await
        .unwrap();
    assert!(env.app_path().join("test-app").exists());

    let remove_cli = Cli {
        command: SubCommand::Remove {
            packages: vec!["test-app".to_string()],
            soft: false,
        },
        prefix: None,
    };

    let ctx = env.ctx().with_dry_run(true);
    dispatch(remove_cli, ctx).await.unwrap();

    // Dry-run remove keeps the DB record, mirroring install's dry-run
    // semantics (it used to be deleted unconditionally).
    assert!(
        env.db().get_repo("test-app").is_some(),
        "dry-run must not remove the db entry"
    );
    assert!(
        env.app_path().join("test-app").exists(),
        "dry-run uninstall should not delete files"
    );
}
