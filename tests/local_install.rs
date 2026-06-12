mod common;

use bpm::{
    cli::{Cli, SortParam, SubCommand},
    command::dispatch,
    storage::db::DbOperation,
};
use common::*;

fn install_cli(pkg: &str, local: &std::path::Path, dry_run: bool) -> Cli {
    Cli {
        command: SubCommand::Install {
            packages: vec![pkg.to_string()],
            bin_name: None,
            local: Some(local.to_path_buf()),
            quiet: true,
            one_bin: false,
            prefer_gnu: false,
            dry_run,
            pre_release: false,
            interactive: false,
            filter: vec![],
            name: None,
            sort: SortParam::default(),
        },
        config: None,
    }
}

fn remove_cli(pkg: &str, soft: bool) -> Cli {
    Cli {
        command: SubCommand::Remove {
            packages: vec![pkg.to_string()],
            soft,
        },
        config: None,
    }
}

#[tokio::test]
async fn full_lifecycle_install_then_remove() {
    let env = TestEnv::new();
    let zip_path = env.tmp().join("test-app.zip");
    #[cfg(windows)]
    create_test_zip(&zip_path, &[("test-app.exe", b"fake binary content")]);
    #[cfg(not(windows))]
    create_test_zip(&zip_path, &[("test-app", b"fake binary content")]);

    dispatch(install_cli("test-app", &zip_path, false), env.ctx())
        .await
        .unwrap();

    let db = env.db();
    let repo = db.get_repo("test-app").unwrap();
    assert_eq!(repo.name, "test-app");
    assert!(!repo.installed_files.is_empty());

    let app_dir = env.app_path().join("test-app");
    assert!(app_dir.exists());
    #[cfg(windows)]
    assert!(app_dir.join("test-app.exe").exists());
    #[cfg(not(windows))]
    assert!(app_dir.join("test-app").exists());

    #[cfg(windows)]
    {
        assert!(env.bin_path().join("test-app.exe").exists());
        assert!(env.bin_path().join("test-app.shim").exists());
    }

    dispatch(remove_cli("test-app", false), env.ctx())
        .await
        .unwrap();

    assert!(env.db().get_repo("test-app").is_none());
    assert!(!app_dir.exists());
}

#[tokio::test]
async fn install_from_zip_with_wrapping_directory() {
    let env = TestEnv::new();
    let zip_path = env.tmp().join("wrapped.zip");
    #[cfg(windows)]
    create_test_zip(
        &zip_path,
        &[
            ("test-app-1.0/test-app.exe", b"binary"),
            ("test-app-1.0/README.md", b"readme"),
            ("test-app-1.0/config.toml", b"config"),
        ],
    );
    #[cfg(not(windows))]
    create_test_zip(
        &zip_path,
        &[
            ("test-app-1.0/test-app", b"binary"),
            ("test-app-1.0/README.md", b"readme"),
            ("test-app-1.0/config.toml", b"config"),
        ],
    );

    dispatch(install_cli("test-app", &zip_path, false), env.ctx())
        .await
        .unwrap();

    let app_dir = env.app_path().join("test-app");
    #[cfg(windows)]
    assert!(app_dir.join("test-app.exe").exists());
    #[cfg(not(windows))]
    assert!(app_dir.join("test-app").exists());
    assert!(app_dir.join("README.md").exists());
    assert!(app_dir.join("config.toml").exists());

    #[cfg(windows)]
    {
        assert!(env.bin_path().join("test-app.exe").exists());
        assert!(env.bin_path().join("test-app.shim").exists());
    }

    assert!(env.db().get_repo("test-app").is_some());

    dispatch(remove_cli("test-app", false), env.ctx())
        .await
        .unwrap();
    assert!(env.db().get_repo("test-app").is_none());
    assert!(!app_dir.exists());
}

#[tokio::test]
async fn install_duplicate_package_is_skipped() {
    let env = TestEnv::new();
    let zip_path = env.tmp().join("test-app.zip");
    create_test_zip(&zip_path, &[("test-app", b"binary")]);

    dispatch(install_cli("test-app", &zip_path, false), env.ctx())
        .await
        .unwrap();

    let first_files = env
        .db()
        .get_repo("test-app")
        .unwrap()
        .installed_files
        .clone();

    dispatch(install_cli("test-app", &zip_path, false), env.ctx())
        .await
        .unwrap();

    let repo = env.db().get_repo("test-app").unwrap();
    assert_eq!(
        repo.installed_files, first_files,
        "installed_files should remain unchanged after duplicate install"
    );

    dispatch(remove_cli("test-app", false), env.ctx())
        .await
        .unwrap();
}

#[tokio::test]
async fn install_multiple_local_packages_fails() {
    let env = TestEnv::new();
    let zip_path = env.tmp().join("test-app.zip");
    create_test_zip(&zip_path, &[("test-app", b"binary")]);

    let cli = Cli {
        command: SubCommand::Install {
            packages: vec!["test-app".to_string(), "other".to_string()],
            bin_name: None,
            local: Some(zip_path),
            quiet: true,
            one_bin: false,
            prefer_gnu: false,
            dry_run: false,
            pre_release: false,
            interactive: false,
            filter: vec![],
            name: None,
            sort: SortParam::default(),
        },
        config: None,
    };

    let result = dispatch(cli, env.ctx()).await;
    assert!(result.is_err());
}
