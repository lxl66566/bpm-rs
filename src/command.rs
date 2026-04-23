use anyhow::Result;
use log::{error, info};

use crate::{
    cli::{Cli, SortParam, SubCommand},
    context::Context,
    installation::{Installation, download, unzip},
    search::{Searchable, SearchableSequence},
    storage::{Repo, RepoList, db::DbOperation},
};

pub async fn dispatch(cli: Cli, ctx: Context) -> Result<()> {
    match cli.command {
        SubCommand::Install {
            packages,
            bin_name,
            local,
            quiet,
            one_bin,
            prefer_gnu,
            dry_run,
            interactive,
            filter,
            sort,
        } => {
            cli_install(
                &ctx.with_dry_run(dry_run).with_quiet(quiet),
                packages,
                bin_name,
                local,
                one_bin,
                prefer_gnu,
                interactive,
                filter,
                sort,
            )
            .await
        }
        SubCommand::Remove { packages, soft } => cli_remove(&ctx, packages, soft).await,
        SubCommand::Update { packages, local } => cli_update(&ctx, packages, local).await,
        #[cfg(windows)]
        SubCommand::Alias { new_name, old_name } => cli_alias(&ctx, old_name, new_name).await,
        SubCommand::Info { packages } => cli_info(&ctx, packages).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn cli_install(
    ctx: &Context,
    packages: Vec<String>,
    bin_name: Option<String>,
    local: Option<std::path::PathBuf>,
    one_bin: bool,
    prefer_gnu: bool,
    interactive: bool,
    filter: Vec<String>,
    sort: SortParam,
) -> Result<()> {
    if interactive && ctx.quiet {
        anyhow::bail!("Cannot use both --interactive and --quiet.");
    }

    if local.is_some() && packages.len() > 1 {
        anyhow::bail!("Cannot install multiple packages from local.");
    }

    if !ctx.dry_run {
        #[cfg(unix)]
        crate::utils::check_root()?;
    }

    let db = ctx.db()?;
    let mut repo_list = build_repo_list(packages, bin_name, one_bin, prefer_gnu, filter);

    if local.is_none() {
        repo_list = repo_list.search_all(ctx.quiet, interactive, sort).await?;
    }

    for mut repo in repo_list.0 {
        if !ctx.dry_run && db.get_repo(&repo.name).is_some() {
            info!("{} is already installed, skipping.", repo.name);
            continue;
        }

        match install_single(ctx, &mut repo, local.as_deref()).await {
            Ok(()) => {
                if !ctx.dry_run {
                    db.insert_repo(repo)?;
                }
            }
            Err(e) => {
                error!("Failed to install `{}`: {e}", repo.name);
                if !ctx.dry_run {
                    info!("Restoring...");
                    let mut repo_for_cleanup = repo.clone();
                    let _ = repo_for_cleanup.uninstall(ctx);
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

fn build_repo_list(
    packages: Vec<String>,
    bin_name: Option<String>,
    one_bin: bool,
    prefer_gnu: bool,
    filter: Vec<String>,
) -> RepoList {
    packages
        .into_iter()
        .map(|p| {
            let mut repo = Repo::from(p.as_str());
            if let Some(ref bn) = bin_name {
                repo = repo.with_bin_name(bn.clone());
            }
            repo.one_bin = one_bin;
            repo.prefer_gnu = prefer_gnu;
            if !filter.is_empty() {
                repo.asset_filter = filter.clone();
            }
            repo
        })
        .collect::<Vec<_>>()
        .into()
}

async fn install_single(
    ctx: &Context,
    repo: &mut Repo,
    local_path: Option<&std::path::Path>,
) -> Result<()> {
    let tmp_dir = tempfile::tempdir()?;
    let tmp_path = tmp_dir.path();

    let main_path = if let Some(local) = local_path {
        let dest = tmp_path.join("local");
        std::fs::create_dir_all(&dest)?;
        std::fs::copy(local, dest.join(local.file_name().unwrap()))?;
        unzip::unzip(
            dest.join(local.file_name().unwrap()),
            tmp_path.join("extracted"),
        )?
    } else {
        let downloaded = download::download(vec![&*repo], tmp_path).await?;
        if downloaded.is_empty() {
            anyhow::bail!("No files downloaded for {}", repo.name);
        }
        unzip::unzip(&downloaded[0], tmp_path.join("extracted"))?
    };

    repo.install(&main_path, ctx)?;
    Ok(())
}

async fn cli_remove(ctx: &Context, packages: Vec<String>, soft: bool) -> Result<()> {
    #[cfg(unix)]
    crate::utils::check_root()?;

    let db = ctx.db()?;
    let mut failed = Vec::new();

    for name in &packages {
        match db.get_repo(name) {
            Some(mut repo) => {
                info!(
                    "Removing `{name}`{}...",
                    if soft { " in soft mode" } else { "" }
                );
                if !soft {
                    repo.uninstall(ctx)?;
                }
                db.remove_repo(name)?;
                info!("`{name}` removed successfully.");
            }
            None => {
                info!("Package `{name}` is not installed.");
                failed.push(name.clone());
            }
        }
    }

    info!(
        "Remove complete. Total: {}, Success: {}",
        packages.len(),
        packages.len() - failed.len()
    );
    if !failed.is_empty() {
        info!("Failed: {failed:?}");
    }
    Ok(())
}

async fn cli_update(
    ctx: &Context,
    packages: Vec<String>,
    _local: Option<std::path::PathBuf>,
) -> Result<()> {
    #[cfg(unix)]
    crate::utils::check_root()?;

    let db = ctx.db()?;
    let mut failed = Vec::new();
    let all_repos = db.get_repo_list();

    let to_update: Vec<Repo> = if packages.is_empty() {
        all_repos.0
    } else {
        packages
            .iter()
            .filter_map(|name| {
                let repo = db.get_repo(name);
                if repo.is_none() {
                    failed.push(name.clone());
                    error!("Package `{name}` not found.");
                }
                repo
            })
            .collect()
    };

    let total = to_update.len();
    for mut repo in to_update {
        let repo_name = repo.name.clone();
        info!("Updating `{repo_name}`...");
        match repo.update_asset().await {
            Some((old, new)) => {
                info!("`{repo_name}` has an update: {old} -> {new}. Updating...");
                match install_single(ctx, &mut repo, None).await {
                    Ok(()) => {
                        repo.version = Some(new);
                        db.insert_repo(repo)?;
                        info!("`{repo_name}` updated successfully.");
                    }
                    Err(e) => {
                        failed.push(repo_name.clone());
                        error!("Failed to update {repo_name}: {e}");
                    }
                }
            }
            None => {
                info!("`{repo_name}` is already up to date.");
            }
        }
    }

    info!(
        "Update complete. Total: {total}, Success: {}",
        total - failed.len()
    );
    if !failed.is_empty() {
        info!("Failed: {failed:?}");
    }
    Ok(())
}

#[cfg(windows)]
async fn cli_alias(ctx: &Context, old_name: String, new_name: String) -> Result<()> {
    if old_name == new_name {
        anyhow::bail!("Alias name cannot be the same as the original.");
    }

    let db = ctx.db()?;
    let bin_path = ctx.bin_path();

    let files: Vec<_> = std::fs::read_dir(&bin_path)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with(&old_name))
                .unwrap_or(false)
        })
        .collect();

    if files.is_empty() {
        anyhow::bail!("Script `{old_name}` not found.");
    }

    let mut count = 0u32;
    let all_repos = db.get_repo_list();
    for repo in &all_repos.0 {
        for file in &repo.installed_files {
            let path = std::path::Path::new(file);
            if !path.exists() || path.is_dir() {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if stem == old_name && (ext == "lnk" || ext == "cmd" || ext.is_empty()) {
                let new_path = path.with_file_name(format!("{new_name}.{}", ext));
                std::fs::rename(path, &new_path)?;
                count += 1;
                if count >= 3 {
                    break;
                }
            }
        }
        if count >= 3 {
            break;
        }
    }

    info!("Alias `{old_name}` to `{new_name}`.");
    Ok(())
}

async fn cli_info(ctx: &Context, packages: Vec<String>) -> Result<()> {
    let db = ctx.db()?;
    if packages.is_empty() {
        let all = db.get_repo_list();
        println!("{all}");
    } else {
        for name in &packages {
            match db.get_repo(name) {
                Some(repo) => println!("{repo}"),
                None => error!("Package `{name}` not found."),
            }
        }
    }
    Ok(())
}
