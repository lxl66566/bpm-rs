use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Result, anyhow, bail, ensure};
use log::{debug, error, info, warn};
use url::Url;

use crate::{
    cli::InstallOptions,
    context::Context,
    installation::{Installation, cache::DownloadCache, download, unzip},
    search::{Searchable, UpdateCheck},
    storage::{InstalledFiles, LibcPref, Repo, RepoList, db::DbOperation},
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
                    repo.installed_time = Some(SystemTime::now());
                    db.insert_repo(repo)?;
                }
            },
            Err(e) => {
                error!("Failed to install `{}`: {e}", repo.name);
                // Fresh install: the empty baseline makes the rollback remove
                // everything this attempt tracked.
                rollback_failed_install(ctx, &repo, &InstalledFiles::new());
                return Err(e);
            },
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
            // Every repo here had no url when spawned (search_indices filters
            // on that), so empty items can only mean the search found
            // nothing. `ask` reports the precise error; silently continuing
            // used to end in the misleading "repo_owner not set".
            let items = search_result?;
            repos[i].ask(items, ctx.quiet)?;
        }
    }

    // Phase 3: get_asset. Interactive selection must run sequentially:
    // parallel prompts (FuzzySelect) would fight over the terminal. This
    // mirrors the sequential `ask` in Phase 2.
    if interactive {
        for repo in &mut repos {
            repo.get_asset(interactive).await?;
        }
    } else {
        let asset_indices: Vec<usize> = repos
            .iter()
            .enumerate()
            .filter(|(_, r)| r.asset.is_none())
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
    }

    // Phase 4: restore archives cached from a previous failed install, then
    // batch-download the rest (trauma handles internal parallelism).
    let download_tmp = tempfile::tempdir()?;
    let cache = ctx.download_cache();
    let downloaded = fetch_archives(&cache, &repos, download_tmp.path(), ctx.quiet).await?;
    ensure!(!downloaded.is_empty(), "No files downloaded.");

    // Phase 5: Unzip and install each repo (two-phase: install all, then commit DB)
    let mut installed: Vec<Repo> = Vec::with_capacity(repos.len());
    // (repo name, asset URL) pairs for cache bookkeeping below; `repos` is
    // consumed by the loop, so the URLs must be captured up front.
    let assets: Vec<(String, String)> = repos
        .iter()
        .filter_map(|r| Some((r.name.clone(), r.asset.clone()?)))
        .collect();
    for mut repo in repos {
        // Safe point for graceful Ctrl-C cancellation: bailing here runs
        // the rollback below instead of leaving orphan files behind.
        if crate::utils::cancelled() {
            cache_failed_batch(&cache, &downloaded, &assets, None, ctx.dry_run);
            bail!("cancelled by user, rolling back");
        }

        let main_path = match extract_downloaded(&downloaded, download_tmp.path(), &repo) {
            Ok(path) => path,
            Err(e) => {
                // Corrupt archive: cache the rest of the batch but never this
                // asset (nor any stale copy of it), so a retry re-downloads.
                cache_failed_batch(&cache, &downloaded, &assets, Some(&repo.name), ctx.dry_run);
                return Err(e);
            },
        };

        match repo.install(&main_path, ctx) {
            Ok(()) => {
                info!("`{}` installed successfully.", repo.name);
                repo.interactive = interactive;
                repo.installed_time = Some(SystemTime::now());
                installed.push(repo);
            },
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
                }
                rollback_failed_install(ctx, &repo, &InstalledFiles::new());
                // The batch is all-or-nothing: cache every archive so the
                // retry skips re-downloading them.
                cache_failed_batch(&cache, &downloaded, &assets, None, ctx.dry_run);
                return Err(e);
            },
        }
    }

    // Commit: all installations succeeded, now persist to DB and drop the
    // packages' cached archives (a successful install caches nothing).
    if !ctx.dry_run {
        for repo in &installed {
            cache.clear(&repo.name);
        }
        for repo in installed {
            db.insert_repo(repo)?;
        }
    }

    Ok(())
}

/// Extract `repo`'s entry from a completed download batch into `tmp`.
fn extract_downloaded(
    downloaded: &[(String, PathBuf)],
    tmp: &Path,
    repo: &Repo,
) -> Result<PathBuf> {
    let file = downloaded
        .iter()
        .find(|(name, _)| *name == repo.name)
        .map(|(_, path)| path)
        .ok_or_else(|| anyhow!("No downloaded file for {}", repo.name))?;
    let extracted = tmp.join(format!("{}_extracted", repo.name));
    unzip::unzip(file, extracted)
}

/// Resolve the archives for `repos`: cached entries from a previous failed
/// install are copied back under their canonical download filename, the
/// rest are batch-downloaded into `download_tmp`. Returns one
/// `(repo name, archive path)` pair per repo, cache hits and fresh downloads
/// alike.
async fn fetch_archives(
    cache: &DownloadCache,
    repos: &[Repo],
    download_tmp: &Path,
    quiet: bool,
) -> Result<Vec<(String, PathBuf)>> {
    let mut downloaded: Vec<(String, PathBuf)> = Vec::with_capacity(repos.len());
    let mut to_download: Vec<&Repo> = Vec::new();
    for repo in repos {
        // A repo without asset falls through to download(), which reports
        // the precise error.
        if let Some(url) = repo.asset.as_deref()
            && let Ok(parsed) = Url::parse(url)
            && let Some(entry) = cache.get(&repo.name, url)
        {
            // Restore under the canonical filename: extraction stages
            // downstream (e.g. naming of decompressed single files) must
            // not see the cache key.
            let dest = download_tmp.join(download::download_filename(&repo.name, &parsed)?);
            // Hard-link when possible: the cache and the tempdir usually
            // share a volume, and archives can be large.
            fs::hard_link(&entry, &dest).or_else(|_| fs::copy(&entry, &dest).map(|_| ()))?;
            info!("Using cached download of `{}`.", repo.name);
            downloaded.push((repo.name.clone(), dest));
        } else {
            to_download.push(repo);
        }
    }
    if !to_download.is_empty() {
        downloaded.extend(download::download(to_download, download_tmp, quiet).await?);
    }
    Ok(downloaded)
}

/// Failure stage deciding the cache bookkeeping of a package.
#[derive(Debug, Clone, Copy)]
enum CacheFailure {
    /// The archive failed to extract: it is never cached (likely corrupt),
    /// and a stale entry of the same asset is dropped so a retry
    /// re-downloads it.
    Extract,
    /// Extraction succeeded but the install did not: keep the archive so
    /// a retry skips re-downloading it.
    Install,
}

/// Cache bookkeeping of one package after a failed install/update. No-op
/// on dry-run.
fn cache_failure(
    cache: &DownloadCache,
    downloaded: &[(String, PathBuf)],
    name: &str,
    url: &str,
    kind: CacheFailure,
    dry_run: bool,
) {
    if dry_run {
        return;
    }
    match kind {
        CacheFailure::Extract => cache.invalidate(name, url),
        CacheFailure::Install => {
            let Some((_, path)) = downloaded.iter().find(|(n, _)| n == name) else {
                return;
            };
            if let Err(e) = cache.store(name, url, path) {
                warn!("Failed to cache download of `{name}`: {e}");
            }
        },
    }
}

/// After a failed install batch, cache every downloaded archive so a
/// retry skips re-downloading it. `extract_failed` names the repo whose
/// archive could not be extracted: it is left uncached and any stale copy
/// of it is dropped. No-op on dry-run.
fn cache_failed_batch(
    cache: &DownloadCache,
    downloaded: &[(String, PathBuf)],
    assets: &[(String, String)],
    extract_failed: Option<&str>,
    dry_run: bool,
) {
    for (name, url) in assets {
        let kind = if Some(name.as_str()) == extract_failed {
            CacheFailure::Extract
        } else {
            CacheFailure::Install
        };
        cache_failure(cache, downloaded, name, url, kind, dry_run);
    }
}

/// Cache bookkeeping of `repo` after a failed update. No-op on dry-run.
fn cache_update_failure(
    cache: &DownloadCache,
    downloaded: &[(String, PathBuf)],
    repo: &Repo,
    kind: CacheFailure,
    dry_run: bool,
) {
    if let Some(url) = repo.asset.as_deref() {
        cache_failure(cache, downloaded, &repo.name, url, kind, dry_run);
    }
}

/// Roll back a failed install attempt: uninstall only the files the attempt
/// added on top of `before` (the tracked set when it started), restoring
/// `.old` backups along the way, so no orphan files are left that neither
/// the DB nor the previous record knows about (review P1-13).
///
/// Paths already in `before` belong to the previous install and are kept:
/// the DB still records them, so a full uninstall would trade orphans for
/// holes that doctor then reports. Files the attempt overwrote stay under
/// their tracked path (the same state a successful update leaves behind).
/// For a fresh install (`before` empty) this removes everything the attempt
/// tracked. No-op on dry-run (nothing was written).
fn rollback_failed_install(ctx: &Context, repo: &Repo, before: &InstalledFiles) {
    if ctx.dry_run {
        return;
    }
    info!("Rolling back the failed install of `{}`...", repo.name);
    let mut cleanup = repo.clone();
    cleanup.installed_files = repo.installed_files.difference(before).cloned().collect();
    let _ = cleanup.uninstall(ctx);
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
    // Dedupe (preserving order) so `bpm i foo foo` runs once.
    let mut seen = std::collections::HashSet::new();
    packages
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
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
    local_path: Option<&Path>,
) -> Result<()> {
    let tmp_dir = tempfile::tempdir()?;
    let tmp_path = tmp_dir.path();

    let main_path = if let Some(local) = local_path {
        let is_dir = local.is_dir();
        if is_dir && ctx.dry_run {
            // Dry-run never writes to src (both installers guard their
            // moves), so read the directory in place instead of copying
            // potentially gigabytes into the temp dir.
            local.to_path_buf()
        } else if is_dir {
            // Copy (never move) the directory so the user's source stays
            // intact — the Windows installer moves directory contents.
            let dest = tmp_path.join("local");
            let mut options = fs_extra::dir::CopyOptions::new();
            options.content_only = true;
            options.overwrite = true;
            fs::create_dir_all(&dest)?;
            fs_extra::dir::copy(local, &dest, &options)?;
            dest
        } else {
            // `unzip` leaves the input intact, so extract straight from the
            // user's file; the output goes to the temp dir, also on
            // dry-run — contents cannot be inspected without extracting.
            unzip::unzip(local, tmp_path.join("extracted"))?
        }
    } else {
        let downloaded = download::download(vec![&*repo], tmp_path, ctx.quiet).await?;
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
                if soft {
                    " in soft mode"
                } else {
                    ""
                }
            );
            if !soft {
                repo.uninstall(ctx)?;
            }
            // Mirror install's dry-run semantics: no DB writes either.
            if !ctx.dry_run {
                db.remove_repo(name)?;
                // A removed package never retries: its cached archives
                // would only accumulate.
                ctx.download_cache().clear(name);
            }
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
    local: Option<PathBuf>,
    interactive: bool,
) -> Result<()> {
    let db = ctx.db()?;
    let mut failed = Vec::new();
    let mut not_found = Vec::new();
    let all_repos = db.get_repo_list();

    let to_update: Vec<Repo> = if packages.is_empty() {
        all_repos.into_inner()
    } else {
        packages
            .iter()
            .filter_map(|name| {
                let repo = db.get_repo(name);
                if repo.is_none() {
                    not_found.push(name.clone());
                    error!("Package `{name}` not found.");
                }
                repo
            })
            .collect()
    };

    // Separate local and remote repos
    let (local_repos, remote_repos): (Vec<Repo>, Vec<Repo>) =
        to_update.into_iter().partition(|r| r.local);

    let total = local_repos.len() + remote_repos.len();

    // Phase 1: Handle local repos sequentially (need --local flag)
    for mut repo in local_repos {
        if crate::utils::cancelled() {
            let repo_name = repo.name.clone();
            error!("Cancelled by user, skipping update for {repo_name}.");
            failed.push(repo_name);
            continue;
        }
        let repo_name = repo.name.clone();
        if let Some(local_path) = &local {
            info!("Updating `{repo_name}` from local path...");
            let before = repo.installed_files.clone();
            match install_single(ctx, &mut repo, Some(local_path)).await {
                Ok(()) => {
                    repo.installed_time = Some(SystemTime::now());
                    db.insert_repo(repo)?;
                    info!("`{repo_name}` updated successfully.");
                },
                Err(e) => {
                    failed.push(repo_name.clone());
                    error!("Failed to update {repo_name}: {e}");
                    rollback_failed_install(ctx, &repo, &before);
                },
            }
        } else {
            error!(
                "`{repo_name}` was installed from a local path. Use --local to specify the path \
                 for update."
            );
            failed.push(repo_name);
        }
    }

    // Phase 2: Parallel update checks (network-bound). `check_update` never
    // touches the terminal, so spawning is safe; interactive asset selection
    // is deferred to Phase 3 — parallel prompts (FuzzySelect) would fight
    // over the terminal, the same reasoning as cli_install Phase 3.
    let mut checked: Vec<(usize, Repo, UpdateCheck)> = Vec::new();
    if !remote_repos.is_empty() {
        let mut tasks = tokio::task::JoinSet::new();
        for (i, mut repo) in remote_repos.into_iter().enumerate() {
            tasks.spawn(async move {
                let check = repo.check_update().await;
                (i, repo, check)
            });
        }
        while let Some(res) = tasks.join_next().await {
            checked.push(res?);
        }
        // JoinSet yields results in completion order; restore the input order
        // so the log output does not depend on network timing.
        checked.sort_unstable_by_key(|(i, ..)| *i);
    }

    // Phase 3: Sequential reporting and asset selection (the only stage that
    // may prompt).
    let mut to_install: Vec<Repo> = Vec::new();
    for (_, mut repo, check) in checked {
        let repo_name = repo.name.clone();
        match check {
            UpdateCheck::Update { old, new } => {
                info!("`{repo_name}` has an update: {old} -> {new}. Updating...");
                to_install.push(repo);
            },
            UpdateCheck::PendingSelection { old, release } => {
                let use_interactive = interactive || repo.interactive;
                match repo.select_update_asset(&release, use_interactive) {
                    Ok(()) => {
                        info!(
                            "`{repo_name}` has an update: {old} -> {}. Updating...",
                            release.tag
                        );
                        to_install.push(repo);
                    },
                    Err(e) => {
                        failed.push(repo_name.clone());
                        error!("Failed to check update for {repo_name}: {e}");
                    },
                }
            },
            UpdateCheck::UpToDate { current, latest } => {
                info!(
                    "`{repo_name}` is already up to date (current: {current}, latest: {latest})."
                );
            },
            UpdateCheck::Failed(e) => {
                failed.push(repo_name.clone());
                error!("Failed to check update for {repo_name}: {e}");
            },
        }
    }

    // Phase 4 + 5: restore cached archives / batch download (trauma
    // parallelizes internally), then unzip and install sequentially —
    // mirrors the cli_install pipeline instead of one tempdir + download +
    // install per package.
    if !to_install.is_empty() {
        let download_tmp = tempfile::tempdir()?;
        let cache = ctx.download_cache();
        match fetch_archives(&cache, &to_install, download_tmp.path(), ctx.quiet).await {
            Ok(downloaded) => {
                for mut repo in to_install {
                    if crate::utils::cancelled() {
                        error!("Cancelled by user, skipping update for {}.", repo.name);
                        failed.push(repo.name.clone());
                        // Keep the not-installed archive so a retry skips
                        // re-downloading it, mirroring cli_install.
                        // `cancelled` latches, so every remaining repo
                        // reaches this branch too.
                        cache_update_failure(
                            &cache,
                            &downloaded,
                            &repo,
                            CacheFailure::Install,
                            ctx.dry_run,
                        );
                        continue;
                    }
                    let repo_name = repo.name.clone();
                    let before = repo.installed_files.clone();
                    let main_path =
                        match extract_downloaded(&downloaded, download_tmp.path(), &repo) {
                            Ok(path) => path,
                            Err(e) => {
                                failed.push(repo_name.clone());
                                error!("Failed to update {repo_name}: {e}");
                                // Corrupt archive: drop the cached copy so a
                                // retry re-downloads instead of hitting the
                                // poison entry again.
                                cache_update_failure(
                                    &cache,
                                    &downloaded,
                                    &repo,
                                    CacheFailure::Extract,
                                    ctx.dry_run,
                                );
                                continue;
                            },
                        };
                    match repo.install(&main_path, ctx) {
                        Ok(()) => {
                            repo.installed_time = Some(SystemTime::now());
                            db.insert_repo(repo)?;
                            // A successful update leaves nothing cached.
                            if !ctx.dry_run {
                                cache.clear(&repo_name);
                            }
                            info!("`{repo_name}` updated successfully.");
                        },
                        Err(e) => {
                            failed.push(repo_name.clone());
                            error!("Failed to update {repo_name}: {e}");
                            cache_update_failure(
                                &cache,
                                &downloaded,
                                &repo,
                                CacheFailure::Install,
                                ctx.dry_run,
                            );
                            rollback_failed_install(ctx, &repo, &before);
                        },
                    }
                }
            },
            // The download batch fails as a whole; without per-package
            // granularity every pending update is reported failed and the
            // summary / non-zero exit below still surface it.
            Err(e) => {
                error!("Failed to download updates: {e:#}");
                failed.extend(to_install.iter().map(|r| r.name.clone()));
            },
        }
    }

    // `failed` only contains repos that were part of to_update, so
    // total - failed.len() can never underflow (not_found is separate).
    info!(
        "Update complete. Total: {total}, Success: {}",
        total - failed.len()
    );
    if !failed.is_empty() {
        error!("Failed: {failed:?}");
    }
    // A partial failure must surface as a non-zero exit code, otherwise
    // scripts cannot detect it (not-found packages included).
    let mut failure = failed;
    failure.extend(not_found);
    if !failure.is_empty() {
        bail!("Update failed for {} package(s).", failure.len());
    }
    Ok(())
}

#[cfg(windows)]
pub async fn cli_alias(ctx: &Context, old_name: String, new_name: String) -> Result<()> {
    use std::ffi::OsStr;

    ensure!(
        old_name != new_name,
        "Alias name cannot be the same as the original."
    );

    let db = ctx.db()?;
    let bin_path = ctx.bin_path();

    let mut repo = db
        .get_repo(&old_name)
        .ok_or_else(|| anyhow!("Package `{old_name}` is not installed."))?;

    // Only rename shim files owned by this package (its tracked files under
    // bin). The previous implementation scanned every package's files and
    // could rename unrelated ones that happened to share the stem.
    let owned: Vec<PathBuf> = repo
        .installed_files
        .iter()
        .filter(|p| {
            p.starts_with(&bin_path) && p.file_stem() == Some(OsStr::new(old_name.as_str()))
        })
        .cloned()
        .collect();
    ensure!(
        !owned.is_empty(),
        "No shim files for `{old_name}` found in {}.",
        bin_path.display()
    );

    for path in owned {
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let new_path = path.with_file_name(format!("{new_name}{ext}"));
        std::fs::rename(&path, &new_path)?;
        // Keep tracking in sync so doctor/remove stay correct.
        repo.installed_files.remove(&path);
        repo.installed_files.insert(new_path);
    }

    db.insert_repo(repo)?;
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
        // Always emit one valid JSON document (an array); concatenating
        // per-package documents is not valid JSON.
        let repos: Vec<Repo> = if packages.is_empty() {
            db.get_repo_list().into_inner()
        } else {
            packages
                .iter()
                .filter_map(|name| {
                    let repo = db.get_repo(name);
                    if let Some(repo) = repo {
                        Some(repo)
                    } else {
                        error!("Package `{name}` not found.");
                        None
                    }
                })
                .collect()
        };
        println!("{}", serde_json::to_string_pretty(&repos)?);
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
        // Report requested names that are not installed, matching cli_update
        // behavior (they used to be silently dropped).
        let known: Vec<Repo> = all_repos
            .into_inner()
            .into_iter()
            .filter(|r| packages.contains(&r.name))
            .collect();
        for name in &packages {
            if !known.iter().any(|r| &r.name == name) {
                error!("Package `{name}` not found.");
            }
        }
        known
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
            },
            Err(e) => {
                error!("Failed to check {}: {e}", repo.name);
            },
        }
    }
    Ok(())
}

pub async fn cli_doctor(ctx: &Context) -> Result<()> {
    let db = ctx.db()?;
    let all_repos = db.get_repo_list();
    let mut issues = 0u32;

    // A failed install leaves its archives cached until the next
    // successful one; entries of packages that never retry again are
    // removed here, doctor being the global cleanup point.
    for name in ctx
        .download_cache()
        .clean_orphans(|n| db.get_repo(n).is_some())
    {
        info!("Removed stale download cache of `{name}`.");
    }

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

#[cfg(test)]
mod tests {
    /// A cached entry from a previous failed install must be restored under
    /// the canonical download filename, without touching the network.
    #[tokio::test]
    async fn fetch_archives_restores_cached_entry() {
        use std::fs;

        use super::{Context, Repo, fetch_archives};

        let tmp = tempfile::tempdir().unwrap();
        let ctx = Context::new()
            .with_install_position(tmp.path().join("bpm"))
            .with_db_path(tmp.path().join("db.ron"));
        let url = "https://github.com/o/r/releases/download/v1.0/tool-linux.tar.gz";

        let cache = ctx.download_cache();
        let entry = cache.entry("tool", url);
        fs::create_dir_all(entry.parent().unwrap()).unwrap();
        fs::write(&entry, "archive").unwrap();

        let mut repo = Repo::new("tool");
        repo.asset = Some(url.to_string());

        let download_tmp = tempfile::tempdir().unwrap();
        let fetched = fetch_archives(&cache, &[repo], download_tmp.path(), true)
            .await
            .unwrap();

        assert_eq!(fetched.len(), 1);
        let (name, path) = &fetched[0];
        assert_eq!(name, "tool");
        assert_eq!(path, &download_tmp.path().join("tool.tar.gz"));
        assert_eq!(fs::read(path).unwrap(), b"archive");
    }

    /// Pin the P1-13 rollback contract for a failed update install: files
    /// the attempt added are removed (no orphans outside the DB record),
    /// interrupted writes are restored from their `.old` backup, and files
    /// owned by the previous install survive untouched.
    #[cfg(unix)]
    #[test]
    fn rollback_failed_update_removes_only_added_files() {
        use std::fs;

        use super::{Context, Repo, rollback_failed_install};

        let root = tempfile::tempdir().unwrap();
        let ctx = Context::new().with_prefix(Some(root.path().to_path_buf()));

        // Previous install: tracked set + files on disk (the DB record).
        let bin = root.path().join("bin").join("tool");
        let share = root.path().join("share").join("data.txt");
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::create_dir_all(share.parent().unwrap()).unwrap();
        fs::write(&bin, "v1").unwrap();
        fs::write(&share, "v1").unwrap();

        let mut repo = Repo::new("tool");
        repo.installed_files.insert(bin.clone());
        repo.installed_files.insert(root.path().join("share"));
        repo.installed_files.insert(share.clone());
        let before = repo.installed_files.clone();

        // Failed update attempt state: a new file was written (orphan
        // candidate), a tracked binary was overwritten (old content parked
        // in `tool.old`), and one write was interrupted after its rename
        // (`half.old` exists, `half` does not).
        let added = root.path().join("lib").join("new.so");
        fs::create_dir_all(added.parent().unwrap()).unwrap();
        fs::write(&added, "v2").unwrap();
        fs::write(root.path().join("bin").join("tool.old"), "v1").unwrap();
        fs::write(&bin, "v2").unwrap();
        let half = root.path().join("bin").join("half");
        fs::write(root.path().join("bin").join("half.old"), "v1").unwrap();
        repo.installed_files.insert(root.path().join("lib"));
        repo.installed_files.insert(added.clone());
        repo.installed_files.insert(half.clone());

        rollback_failed_install(&ctx, &repo, &before);

        assert!(!added.exists(), "added-by-attempt file must be removed");
        assert_eq!(
            fs::read_to_string(&half).unwrap(),
            "v1",
            "interrupted write must be restored from its .old backup"
        );
        // The previous install's files survive (the DB still records them).
        assert!(bin.is_file());
        assert!(share.is_file());
    }
}
