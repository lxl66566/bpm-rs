use anyhow::{Result, anyhow, ensure};
use log::{debug, error, info};

use crate::{
    cli::InstallOptions,
    context::Context,
    installation::{Installation, download, unzip},
    search::Searchable,
    storage::{LibcPref, Repo, RepoList, db::DbOperation},
};

pub async fn cli_install(ctx: &Context, opts: InstallOptions) -> Result<()> {
    let InstallOptions {
        packages,
        name,
        bin_name,
        local,
        one_bin,
        prefer_musl,
        interactive,
        filter,
        pre_release,
        sort,
    } = opts;

    ensure!(
        !interactive || !ctx.quiet,
        "Cannot use both --interactive and --quiet."
    );
    ensure!(
        local.is_none() || packages.len() == 1,
        "Cannot install multiple packages from local."
    );
    ensure!(
        name.is_none() || packages.len() == 1,
        "Cannot use --name with multiple packages."
    );

    #[cfg(unix)]
    if !ctx.dry_run && !crate::utils::is_root() && ctx.prefix().is_none() {
        log::info!("Running without root; installing to ~/.local. Use --prefix to override.");
    }

    let db = ctx.db()?;
    let libc_pref = if prefer_musl {
        LibcPref::Musl
    } else {
        LibcPref::Gnu
    };
    let repo_list = build_repo_list(
        packages,
        bin_name.as_ref(),
        one_bin,
        libc_pref,
        filter,
        name,
        pre_release,
    );
    debug!("repo_list: {repo_list:?}");

    // Filter out already installed packages upfront
    let mut repos: Vec<Repo> = repo_list
        .into_inner()
        .into_iter()
        .filter(|repo| {
            if !ctx.dry_run && db.get_repo(&repo.name).is_some() {
                info!("{} is already installed, skipping.", repo.name);
                false
            } else {
                true
            }
        })
        .collect();

    if repos.is_empty() {
        return Ok(());
    }

    // Local install: single package, no parallelism needed
    if let Some(local_path) = local {
        let mut repo = repos.into_iter().next().unwrap();
        match install_single(ctx, &mut repo, Some(&local_path)).await {
            Ok(()) => {
                if !ctx.dry_run {
                    repo.local = true;
                    repo.interactive = interactive;
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
        return Ok(());
    }

    // Phase 1: Parallel search for repos that need it
    let search_indices: Vec<usize> = repos
        .iter()
        .enumerate()
        .filter(|(_, r)| r.url().is_none())
        .map(|(i, _)| i)
        .collect();

    if !search_indices.is_empty() {
        let mut tasks = tokio::task::JoinSet::new();
        for &i in &search_indices {
            let repo = repos[i].clone();
            tasks.spawn(async move { (i, repo.search(sort).await) });
        }

        let mut results = Vec::with_capacity(search_indices.len());
        while let Some(res) = tasks.join_next().await {
            results.push(res?);
        }

        // Phase 2: Sequential ask in original order
        results.sort_by_key(|(i, _)| *i);
        for (i, search_result) in results {
            let items = search_result?;
            if !items.is_empty() {
                repos[i].ask(items, ctx.quiet)?;
            }
        }
    }

    // Phase 3: Parallel get_asset
    let asset_indices: Vec<usize> = repos
        .iter()
        .enumerate()
        .filter(|(_, r)| !interactive || r.asset.is_none())
        .map(|(i, _)| i)
        .collect();

    if !asset_indices.is_empty() {
        let mut tasks = tokio::task::JoinSet::new();
        for &i in &asset_indices {
            let mut repo = std::mem::take(&mut repos[i]);
            tasks.spawn(async move {
                let result = repo.get_asset(interactive).await;
                (i, repo, result)
            });
        }

        while let Some(res) = tasks.join_next().await {
            let (i, repo, result) = res?;
            result?;
            repos[i] = repo;
        }
    }

    // Phase 4: Batch download (trauma handles internal parallelism)
    let download_tmp = tempfile::tempdir()?;
    let repo_refs: Vec<&Repo> = repos.iter().collect();
    let downloaded = download::download(repo_refs, download_tmp.path()).await?;
    ensure!(!downloaded.is_empty(), "No files downloaded.");

    // Phase 5: Unzip and install each repo (two-phase: install all, then commit DB)
    let mut installed: Vec<Repo> = Vec::with_capacity(repos.len());
    for mut repo in repos {
        let file = downloaded
            .iter()
            .find(|(name, _)| *name == repo.name)
            .map(|(_, path)| path)
            .ok_or_else(|| anyhow!("No downloaded file for {}", repo.name))?;

        let extracted = download_tmp.path().join(format!("{}_extracted", repo.name));
        let main_path = unzip::unzip(file, extracted)?;

        match repo.install(&main_path, ctx) {
            Ok(()) => {
                info!("`{}` installed successfully.", repo.name);
                repo.interactive = interactive;
                installed.push(repo);
            }
            Err(e) => {
                error!("Failed to install `{}`: {e}", repo.name);
                if !ctx.dry_run {
                    info!(
                        "Rolling back {} previously installed package(s)...",
                        installed.len()
                    );
                    for mut r in installed {
                        let _ = r.uninstall(ctx);
                    }
                    let mut repo_for_cleanup = repo.clone();
                    let _ = repo_for_cleanup.uninstall(ctx);
                }
                return Err(e);
            }
        }
    }

    // Commit: all installations succeeded, now persist to DB
    if !ctx.dry_run {
        for repo in installed {
            db.insert_repo(repo)?;
        }
    }

    Ok(())
}

fn build_repo_list(
    packages: Vec<String>,
    bin_name: Option<&String>,
    one_bin: bool,
    libc_pref: LibcPref,
    filter: Vec<String>,
    name: Option<String>,
    pre_release: bool,
) -> RepoList {
    packages
        .into_iter()
        .map(move |p| {
            let mut repo = Repo::from(p.as_str());
            if let Some(ref n) = name {
                repo.name.clone_from(n);
            }
            if let Some(bn) = bin_name {
                repo = repo.with_bin_name(bn.clone());
            }
            repo.one_bin = one_bin;
            repo.libc_pref = libc_pref;
            repo.allow_pre = pre_release;
            if !filter.is_empty() {
                repo.asset_filter = filter.clone();
            }
            repo
        })
        .collect::<Vec<_>>()
        .into()
}

pub async fn install_single(
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
        ensure!(
            !downloaded.is_empty(),
            "No files downloaded for {}",
            repo.name
        );
        unzip::unzip(&downloaded[0].1, tmp_path.join("extracted"))?
    };

    repo.install(&main_path, ctx)?;
    Ok(())
}

pub async fn cli_remove(ctx: &Context, packages: Vec<String>, soft: bool) -> Result<()> {
    let db = ctx.db()?;
    let mut failed = Vec::new();

    for name in &packages {
        if let Some(mut repo) = db.get_repo(name) {
            info!(
                "Removing `{name}`{}...",
                if soft { " in soft mode" } else { "" }
            );
            if !soft {
                repo.uninstall(ctx)?;
            }
            db.remove_repo(name)?;
            info!("`{name}` removed successfully.");
        } else {
            info!("Package `{name}` is not installed.");
            failed.push(name.clone());
        }
    }

    info!(
        "Remove complete. Total: {}, Success: {}",
        packages.len(),
        packages.len() - failed.len()
    );
    if !failed.is_empty() {
        error!("Failed: {failed:?}");
    }
    Ok(())
}

pub async fn cli_update(
    ctx: &Context,
    packages: Vec<String>,
    local: Option<std::path::PathBuf>,
    interactive: bool,
) -> Result<()> {
    let db = ctx.db()?;
    let mut failed = Vec::new();
    let all_repos = db.get_repo_list();

    let to_update: Vec<Repo> = if packages.is_empty() {
        all_repos.into_inner()
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

    // Separate local and remote repos
    let (local_repos, remote_repos): (Vec<Repo>, Vec<Repo>) = to_update
        .into_iter()
        .partition(|r| r.local);

    let total = local_repos.len() + remote_repos.len();

    // Phase 1: Handle local repos sequentially (need --local flag)
    for mut repo in local_repos {
        let repo_name = repo.name.clone();
        if let Some(local_path) = &local {
            info!("Updating `{repo_name}` from local path...");
            match install_single(ctx, &mut repo, Some(local_path)).await {
                Ok(()) => {
                    db.insert_repo(repo)?;
                    info!("`{repo_name}` updated successfully.");
                }
                Err(e) => {
                    failed.push(repo_name.clone());
                    error!("Failed to update {repo_name}: {e}");
                }
            }
        } else {
            error!(
                "`{repo_name}` was installed from a local path. Use --local to specify the path for update."
            );
            failed.push(repo_name);
        }
    }

    // Phase 2: Parallel check for updates (network-bound)
    if !remote_repos.is_empty() {
        let mut tasks = tokio::task::JoinSet::new();
        for mut repo in remote_repos {
            let use_interactive = interactive || repo.interactive;
            tasks.spawn(async move {
                let update_result = repo.update_asset(use_interactive).await;
                (repo, update_result)
            });
        }

        // Phase 3: Sequential install for repos that have updates
        while let Some(res) = tasks.join_next().await {
            let (mut repo, update_result) = res?;
            let repo_name = repo.name.clone();

            match update_result {
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
pub async fn cli_alias(ctx: &Context, old_name: String, new_name: String) -> Result<()> {
    ensure!(
        old_name != new_name,
        "Alias name cannot be the same as the original."
    );

    let db = ctx.db()?;
    let bin_path = ctx.bin_path();

    let files: Vec<_> = std::fs::read_dir(&bin_path)?
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(&old_name))
        })
        .collect();

    ensure!(!files.is_empty(), "Script `{old_name}` not found.");

    let mut count = 0u32;
    let all_repos = db.get_repo_list();
    for repo in all_repos.as_slice() {
        for file in &repo.installed_files {
            let path = std::path::Path::new(file);
            if !path.exists() || path.is_dir() {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if stem == old_name && (ext == "exe" || ext == "shim" || ext.is_empty()) {
                let new_path = path.with_file_name(format!("{new_name}.{ext}"));
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

pub async fn cli_info(
    ctx: &Context,
    packages: Vec<String>,
    json: bool,
    outdated: bool,
) -> Result<()> {
    let db = ctx.db()?;

    if json {
        // JSON output for scripting
        if packages.is_empty() {
            let all = db.get_repo_list();
            let json = serde_json::to_string_pretty(all.as_slice())?;
            println!("{json}");
        } else {
            for name in &packages {
                match db.get_repo(name) {
                    Some(repo) => {
                        let json = serde_json::to_string_pretty(&repo)?;
                        println!("{json}");
                    }
                    None => error!("Package `{name}` not found."),
                }
            }
        }
        return Ok(());
    }

    if outdated {
        return check_outdated(ctx, packages).await;
    }

    // Default: table output
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

/// Check which installed packages have available updates.
async fn check_outdated(ctx: &Context, packages: Vec<String>) -> Result<()> {
    let db = ctx.db()?;
    let all_repos = db.get_repo_list();

    let to_check: Vec<Repo> = if packages.is_empty() {
        all_repos.into_inner()
    } else {
        all_repos
            .into_inner()
            .into_iter()
            .filter(|r| packages.contains(&r.name))
            .collect()
    };

    let mut tasks = tokio::task::JoinSet::new();
    for repo in to_check {
        if repo.local {
            info!("`{}` is locally installed, skipping.", repo.name);
            continue;
        }
        tasks.spawn(async move {
            let latest = repo.fetch_latest_release().await;
            (repo, latest)
        });
    }

    while let Some(res) = tasks.join_next().await {
        let (repo, latest_result) = res?;
        match latest_result {
            Ok(release) => {
                let current = repo.version.as_deref().unwrap_or("unknown");
                let latest_tag = &release.tag;
                if current == latest_tag.as_str() {
                    debug!("{} is up to date ({})", repo.name, current);
                } else {
                    info!("{}  {} -> {}", repo.name, current, latest_tag);
                }
            }
            Err(e) => {
                error!("Failed to check {}: {e}", repo.name);
            }
        }
    }
    Ok(())
}

pub async fn cli_doctor(ctx: &Context) -> Result<()> {
    let db = ctx.db()?;
    let all_repos = db.get_repo_list();
    let mut issues = 0u32;

    for repo in all_repos.as_slice() {
        let mut missing = 0u32;
        for file in &repo.installed_files {
            if !file.exists() {
                if missing == 0 {
                    error!("[{}] missing files:", repo.name);
                }
                error!("  {}", file.display());
                missing += 1;
            }
        }
        if missing > 0 {
            issues += missing;
        } else {
            info!("[{}] OK ({} files)", repo.name, repo.installed_files.len());
        }
    }

    if issues > 0 {
        error!("Found {issues} missing file(s) across all packages.");
    } else {
        info!("All packages verified successfully.");
    }
    Ok(())
}
