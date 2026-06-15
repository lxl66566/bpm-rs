mod common;

use bin_package_manager::{
    cli::{Cli, InstallOptions, SortParam, SubCommand},
    dispatch,
    storage::db::DbOperation,
};
use common::*;

fn install_cli(pkg: &str, local: &std::path::Path) -> Cli {
    Cli {
        command: SubCommand::Install {
            opts: InstallOptions {
                packages: vec![pkg.to_string()],
                bin_name: None,
                local: Some(local.to_path_buf()),
                one_bin: false,
                prefer_musl: false,
                interactive: false,
                filter: vec![],
                name: None,
                pre_release: false,
                sort: SortParam::default(),
            },
            quiet: true,
            dry_run: false,
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
async fn soft_remove_keeps_files() {
    let env = TestEnv::new();
    let zip_path = env.tmp().join("test-app.zip");
    create_test_zip(&zip_path, &[("test-app", b"binary")]);

    dispatch(install_cli("test-app", &zip_path), env.ctx())
        .await
        .unwrap();

    let app_dir = env.app_path().join("test-app");
    assert!(app_dir.join("test-app").exists());

    dispatch(remove_cli("test-app", true), env.ctx())
        .await
        .unwrap();

    assert!(
        env.db().get_repo("test-app").is_none(),
        "db entry should be removed after soft remove"
    );
    assert!(
        app_dir.join("test-app").exists(),
        "files should remain after soft remove"
    );
}

#[tokio::test]
async fn remove_non_existent_package_succeeds() {
    let env = TestEnv::new();

    dispatch(remove_cli("ghost-pkg", false), env.ctx())
        .await
        .unwrap();

    assert!(env.db().get_repo("ghost-pkg").is_none());
}

#[tokio::test]
async fn remove_mixed_packages() {
    let env = TestEnv::new();

    let zip_a = env.tmp().join("pkg-a.zip");
    let zip_b = env.tmp().join("pkg-b.zip");
    create_test_zip(&zip_a, &[("pkg-a", b"a")]);
    create_test_zip(&zip_b, &[("pkg-b", b"b")]);

    dispatch(install_cli("pkg-a", &zip_a), env.ctx())
        .await
        .unwrap();
    dispatch(install_cli("pkg-b", &zip_b), env.ctx())
        .await
        .unwrap();

    assert!(env.db().get_repo("pkg-a").is_some());
    assert!(env.db().get_repo("pkg-b").is_some());

    let cli = Cli {
        command: SubCommand::Remove {
            packages: vec!["pkg-a".to_string(), "non-existent".to_string()],
            soft: false,
        },
        config: None,
    };
    dispatch(cli, env.ctx()).await.unwrap();

    assert!(env.db().get_repo("pkg-a").is_none());
    assert!(env.db().get_repo("non-existent").is_none());
    assert!(env.db().get_repo("pkg-b").is_some());
    assert!(!env.app_path().join("pkg-a").exists());
    assert!(env.app_path().join("pkg-b").exists());
}
