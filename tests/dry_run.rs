mod common;

use bpm::{
    cli::{Cli, SortParam, SubCommand},
    command::dispatch,
    storage::db::DbOperation,
};
use common::*;

#[tokio::test]
async fn dry_run_install_does_not_persist() {
    let env = TestEnv::new();
    let zip_path = env.tmp().join("test-app.zip");
    create_test_zip(&zip_path, &[("test-app", b"binary")]);

    let cli = Cli {
        command: SubCommand::Install {
            packages: vec!["test-app".to_string()],
            bin_name: None,
            local: Some(zip_path),
            quiet: true,
            one_bin: false,
            prefer_musl: false,
            dry_run: true,
            pre_release: false,
            interactive: false,
            filter: vec![],
            name: None,
            sort: SortParam::default(),
        },
        config: None,
    };

    dispatch(cli, env.ctx()).await.unwrap();

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

    let install_cli = Cli {
        command: SubCommand::Install {
            packages: vec!["test-app".to_string()],
            bin_name: None,
            local: Some(zip_path),
            quiet: true,
            one_bin: false,
            prefer_musl: false,
            dry_run: false,
            pre_release: false,
            interactive: false,
            filter: vec![],
            name: None,
            sort: SortParam::default(),
        },
        config: None,
    };

    dispatch(install_cli, env.ctx()).await.unwrap();
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
