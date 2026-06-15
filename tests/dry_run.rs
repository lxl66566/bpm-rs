mod common;

use bin_package_manager::{
    cli::{Cli, InstallOptions, SortParam, SubCommand},
    dispatch,
    storage::db::DbOperation,
};
use common::*;

fn install_cli(zip_path: &std::path::Path, dry_run: bool) -> Cli {
    Cli {
        command: SubCommand::Install {
            opts: InstallOptions {
                packages: vec!["test-app".to_string()],
                bin_name: None,
                local: Some(zip_path.to_path_buf()),
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
        config: None,
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
        config: None,
    };

    let ctx = env.ctx().with_dry_run(true);
    dispatch(remove_cli, ctx).await.unwrap();

    assert!(
        env.db().get_repo("test-app").is_none(),
        "db.remove_repo is called regardless of dry_run"
    );
    assert!(
        env.app_path().join("test-app").exists(),
        "dry-run uninstall should not delete files"
    );
}
