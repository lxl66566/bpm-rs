use std::{sync::LazyLock as Lazy, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use backon::{ExponentialBuilder, Retryable};
use dialoguer::{FuzzySelect, Select};
use log::{debug, info};
use serde::Deserialize;
use url::Url;

use crate::{cli::SortParam, storage::Repo, utils::UrlJoinAll};
static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
static REQUEST_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    let mut headers = reqwest::header::HeaderMap::new();

    // GitHub API version header
    headers.insert(
        "X-GitHub-Api-Version",
        reqwest::header::HeaderValue::from_static("2022-11-28"),
    );

    // Read token from GITHUB_TOKEN or GH_TOKEN environment variable
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN"))
        && let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(reqwest::header::AUTHORIZATION, value);
        debug!("Using GitHub token from environment for higher API rate limit");
    }

    reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        // Without timeouts a stalled network would hang bpm forever.
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .default_headers(headers)
        .build()
        .expect("Failed to build HTTP client")
});

// -------- platform-agnostic types --------

/// A release with its downloadable assets, parsed from any release platform
/// (GitHub, GitLab, etc.).
#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone)]
pub struct Asset {
    pub url: String,
}

// -------- GitHub API response types --------
// Typed so schema mismatches surface at compile time instead of as runtime
// Value indexing bugs.

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubSearchResult {
    #[serde(default)]
    items: Vec<GithubRepo>,
}

#[derive(Debug, Deserialize)]
struct GithubRepo {
    html_url: String,
}

// -------- trait: abstraction over release platforms --------

pub trait Searchable {
    /// Search for repositories by name.
    async fn search(&self, sort: SortParam) -> Result<Vec<String>>;
    /// Interactively choose a repository from search results.
    fn ask(&mut self, items: Vec<String>, quiet: bool) -> Result<()>;
    /// Fetch the latest release and select a matching asset.
    async fn get_asset(&mut self, interactive: bool) -> Result<()>;
    /// Check for a newer version, trying the URL-replacement fast path.
    ///
    /// Terminal-free by design so it can run in parallel: when the fast path
    /// misses, [`UpdateCheck::PendingSelection`] defers the asset selection
    /// to the caller — parallel prompts (`FuzzySelect`) would fight over the
    /// terminal.
    async fn check_update(&mut self) -> UpdateCheck;
}

/// Result of an update check.
#[derive(Debug)]
pub enum UpdateCheck {
    /// No newer release found: the current version equals the latest.
    UpToDate { current: String, latest: String },
    /// A newer release is available and ready to install.
    Update { old: String, new: String },
    /// A newer release is available, but the asset still has to be picked
    /// from `release` by the caller via [`Repo::select_update_asset`] —
    /// sequentially, because the selection may prompt.
    PendingSelection { old: String, release: Release },
    /// The update check failed; carries the underlying error.
    Failed(anyhow::Error),
}
// -------- internal helpers (Repo) ----

/// Whether a request error is transient and worth retrying: network-level
/// failures and 5xx server errors. Rate-limit / auth errors are not — retrying
/// them just burns the rate limit faster.
fn is_transient(e: &reqwest::Error) -> bool {
    e.is_connect()
        || e.is_timeout()
        || e.is_request()
        || e.status().is_some_and(|s| s.is_server_error())
}

/// Send a request with exponential backoff (default: 1s/2s/4s + jitter, 3
/// retries) on transient failures. Takes a request *factory* because a
/// retried request must be rebuilt (`RequestBuilder::send` consumes self).
async fn send_with_retry(build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response> {
    (|| build().send())
        .retry(ExponentialBuilder::default().with_jitter())
        .when(is_transient)
        .await
        .map_err(Into::into)
}

impl Repo {
    /// Fetch the latest release from the platform API and parse into a typed
    /// [`Release`]. Platform-specific parsing is isolated here.
    pub(crate) async fn fetch_latest_release(&self) -> Result<Release> {
        let Some(owner) = self.repo_owner.as_deref() else {
            bail!("repo_owner not set");
        };
        let Some(name) = self.repo_name.as_deref() else {
            bail!("repo_name not set");
        };

        // Fast path for the stable case: /releases/latest is a single-item
        // endpoint that already excludes prereleases (and drafts), much
        // cheaper than pulling the release list (per_page=100) just to find
        // the newest stable one. It 404s — or yields an empty tag — when the
        // repo has no stable release; fall back to the list scan then.
        // `allow_pre` keeps the list scan: the endpoint never returns
        // prereleases.
        if !self.allow_pre {
            let api = self
                .site
                .api_base()
                .join_all_str(["repos", owner, name, "releases", "latest"])?;
            if let Some(raw) = fetch_latest_stable(api).await?
                && !raw.tag_name.is_empty()
            {
                return from_github_release(raw);
            }
        }

        let per_page = if self.allow_pre {
            "1"
        } else {
            "100"
        };
        let api = Url::parse_with_params(
            self.site
                .api_base()
                .join_all_str(["repos", owner, name, "releases"])?
                .as_str(),
            [("per_page", per_page)],
        )?;

        debug!("Fetch releases from API: {api}");
        let releases: Vec<GithubRelease> = send_with_retry(|| REQUEST_CLIENT.get(api.clone()))
            .await?
            .error_for_status()
            .with_context(|| format!("GitHub releases API for {owner}/{name}"))?
            .json()
            .await?;

        let raw = if self.allow_pre {
            releases.into_iter().next()
        } else {
            releases.into_iter().find(|r| !r.prerelease)
        };
        let raw = raw.ok_or_else(|| {
            anyhow!("No release asset found for {owner}/{name}. The repo may have no releases.")
        })?;

        from_github_release(raw)
    }

    /// Resolve a [`UpdateCheck::PendingSelection`]: pick the asset from the
    /// fetched release (prompts when `interactive`) and record version and
    /// asset together.
    pub(crate) fn select_update_asset(
        &mut self,
        release: &Release,
        interactive: bool,
    ) -> Result<()> {
        self.asset = Some(self.select_asset(release, interactive)?);
        self.version = Some(release.tag.clone());
        info!("Selected asset: {}", self.asset.as_deref().unwrap_or(""));
        Ok(())
    }

    /// Select the best matching asset from a release.
    fn select_asset(&self, release: &Release, interactive: bool) -> Result<String> {
        if release.assets.is_empty() {
            bail!(
                "No release asset found for {}/{}",
                self.repo_owner.as_deref().unwrap_or("?"),
                self.repo_name.as_deref().unwrap_or("?")
            );
        }

        let urls: Vec<String> = release.assets.iter().map(|a| a.url.clone()).collect();

        let filtered = if self.asset_filter.is_empty() {
            urls
        } else {
            urls.into_iter()
                .filter(|u| self.asset_filter.iter().all(|f| u.contains(f)))
                .collect()
        };

        let selected = architecture_select::select(filtered);

        // Sort preferred libc variant to the front (best-first)
        let preferred_kw = self.libc_pref.keyword();
        let selected = architecture_select::sort_list(
            selected,
            &[(preferred_kw, architecture_select::MatchPos::All)],
            architecture_select::Combination::Any,
            false,
            architecture_select::MatchOrder::MatchedFirst,
        );

        // Sort natively supported formats to the front, unsupported formats
        // (e.g. .deb, .rpm, .dmg, .AppImage) to the back so we pick something
        // bpm can actually extract. On Windows a raw .exe/.msi is directly
        // installable and counts as supported too.
        let mut supported_exts = vec![
            (".tar.gz", architecture_select::MatchPos::End),
            (".tar.xz", architecture_select::MatchPos::End),
            (".tar.zst", architecture_select::MatchPos::End),
            (".tar.bz2", architecture_select::MatchPos::End),
            (".tar", architecture_select::MatchPos::End),
            (".zip", architecture_select::MatchPos::End),
            (".7z", architecture_select::MatchPos::End),
            (".gz", architecture_select::MatchPos::End),
            (".zst", architecture_select::MatchPos::End),
            (".xz", architecture_select::MatchPos::End),
        ];
        if cfg!(windows) {
            supported_exts.insert(0, (".msi", architecture_select::MatchPos::End));
            supported_exts.insert(0, (".exe", architecture_select::MatchPos::End));
        }
        let selected = architecture_select::sort_list(
            selected,
            &supported_exts,
            architecture_select::Combination::Any,
            false,
            architecture_select::MatchOrder::MatchedFirst,
        );

        if interactive && selected.len() > 1 {
            ask_asset_interactive(&selected)
        } else {
            // Every sort above is best-first: take the first, not the last.
            // (Taking the last once picked .pdb over .exe — review P0-1.)
            selected.into_iter().next().context(format!(
                "No valid asset found for '{}'. Try --interactive.",
                self.name
            ))
        }
    }
}

/// Fetch the latest stable release via the single-item `releases/latest`
/// endpoint. `Ok(None)` means the endpoint answered 404 — the repo has no
/// non-prerelease release — and the caller should fall back to scanning the
/// release list.
async fn fetch_latest_stable(api: Url) -> Result<Option<GithubRelease>> {
    debug!("Fetch latest release from API: {api}");
    let resp = send_with_retry(|| REQUEST_CLIENT.get(api.clone())).await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let release = resp
        .error_for_status()
        .with_context(|| format!("GitHub latest-release API: {api}"))?
        .json()
        .await
        .with_context(|| format!("GitHub latest-release API: {api}"))?;
    Ok(Some(release))
}

/// Convert a typed GitHub release into the platform-agnostic [`Release`].
/// (Isolated so adding GitLab etc. only needs a sister conversion.)
fn from_github_release(raw: GithubRelease) -> Result<Release> {
    if raw.assets.is_empty() {
        bail!("No downloadable assets in release for tag {}", raw.tag_name);
    }
    Ok(Release {
        tag: raw.tag_name,
        assets: raw
            .assets
            .into_iter()
            .map(|a| Asset {
                url: a.browser_download_url,
            })
            .collect(),
    })
}

async fn check_url_exists(url: &str) -> Result<bool> {
    // Try HEAD first (lightweight), fall back to ranged GET for CDNs/proxies
    // that don't support HEAD.
    if send_with_retry(|| REQUEST_CLIENT.head(url))
        .await?
        .status()
        .is_success()
    {
        return Ok(true);
    }
    let resp = send_with_retry(|| REQUEST_CLIENT.get(url).header("Range", "bytes=0-0")).await?;
    Ok(resp.status().is_success())
}

// -------- Searchable trait implementation --------
impl Searchable for Repo {
    async fn search(&self, sort: SortParam) -> Result<Vec<String>> {
        if self.url().is_some() {
            debug!("Repo `{}` url already set, skipping search.", self.name);
            return Ok(vec![]);
        }

        let url = Url::parse_with_params(
            self.site
                .api_base()
                .join_all_str(["search", "repositories"])?
                .as_str()
                .trim_matches('/'),
            [
                ("q", format!("{} in:name", self.name).as_str()),
                ("page", "1"),
                ("sort", sort.as_ref()),
            ],
        )?;

        debug!("search url: {url}");
        let response = send_with_retry(|| REQUEST_CLIENT.get(url.clone()))
            .await?
            .error_for_status()
            .context("GitHub search API")?;

        let result: GithubSearchResult = response.json().await?;
        Ok(result.items.into_iter().map(|r| r.html_url).collect())
    }

    fn ask(&mut self, items: Vec<String>, quiet: bool) -> Result<()> {
        if items.is_empty() {
            bail!("No repos found for '{}'", self.name);
        }

        if quiet {
            self.set_by_url(&items[0])?;
            return Ok(());
        }

        let selection = Select::new()
            .with_prompt("Please select the repo you want to install")
            .items(&items)
            .default(0)
            .interact_opt()?;

        let Some(idx) = selection else {
            bail!("User cancelled the repo selection");
        };
        let selected = &items[idx];
        info!("selected repo: {selected}");
        self.set_by_url(selected)?;
        Ok(())
    }

    async fn get_asset(&mut self, interactive: bool) -> Result<()> {
        let release = self.fetch_latest_release().await?;

        self.version = Some(release.tag.clone());
        self.asset = Some(self.select_asset(&release, interactive)?);

        info!("Selected asset: {}", self.asset.as_deref().unwrap_or(""));
        Ok(())
    }

    async fn check_update(&mut self) -> UpdateCheck {
        let Some(old_version) = self.version.clone() else {
            return UpdateCheck::Failed(anyhow!("no version recorded for this repo"));
        };
        let Some(old_asset) = self.asset.clone() else {
            return UpdateCheck::Failed(anyhow!("no asset recorded for this repo"));
        };

        let release = match self.fetch_latest_release().await {
            Ok(release) => release,
            Err(e) => return UpdateCheck::Failed(e),
        };

        if old_version == release.tag {
            return UpdateCheck::UpToDate {
                current: old_version,
                latest: release.tag,
            };
        }

        // Fast path: replace old version string with new one in the asset URL
        // Fast path failure must fall through to full selection, not abort
        // the update: a transient error on the probe should not kill it.
        let candidate = old_asset.replace(&old_version, &release.tag);
        if candidate != old_asset && check_url_exists(&candidate).await.unwrap_or(false) {
            self.version = Some(release.tag.clone());
            self.asset = Some(candidate);
            return UpdateCheck::Update {
                old: old_version,
                new: release.tag,
            };
        }

        // Fallback: full asset selection. Deferred to the caller because it
        // may prompt (FuzzySelect) and must therefore run sequentially.
        UpdateCheck::PendingSelection {
            old: old_version,
            release,
        }
    }
}

fn ask_asset_interactive(assets: &[String]) -> Result<String> {
    let short_names: Vec<&str> = assets
        .iter()
        .map(|a| a.rsplit('/').next().unwrap_or(a))
        .collect();

    // Use FuzzySelect for assets (many items, search is helpful)
    let selection = FuzzySelect::new()
        .with_prompt("Select an asset")
        .items(&short_names)
        .default(0)
        .interact_opt()?;

    let Some(idx) = selection else {
        bail!("User cancelled the asset selection");
    };
    Ok(assets[idx].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_construction() {
        let url = Url::parse("https://api.github.com").unwrap();
        let joined = url.join_all_str(["search", "repositories"]).unwrap();
        assert_eq!(
            joined.as_str(),
            "https://api.github.com/search/repositories"
        );
    }

    #[test]
    fn test_search_quiet_mode() {
        let mut repo = Repo::new("test");
        repo.repo_owner = Some("owner".to_string());
        repo.repo_name = Some("repo".to_string());
        let result = repo.ask(vec!["https://github.com/a/b".to_string()], true);
        assert!(result.is_ok());
    }

    /// Empty search results must fail with the repo name spelled out;
    /// swallowing them led to a misleading `repo_owner not set` later.
    #[test]
    fn test_ask_empty_items_errors() {
        let mut repo = Repo::new("no-such-package");
        let err = repo.ask(vec![], true).unwrap_err();
        assert!(
            err.to_string()
                .contains("No repos found for 'no-such-package'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_select_asset_with_filter() {
        let mut repo = Repo::new("test-pkg");
        repo.repo_owner = Some("owner".to_string());
        repo.repo_name = Some("repo".to_string());

        // Current platform spelled the way release assets usually do.
        let (os_kw, arch_kw) = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => ("win32", "x64"),
            ("windows", "aarch64") => ("win32", "arm64"),
            ("windows", "x86") => ("win32", "386"),
            ("linux", "x86_64") => ("linux", "amd64"),
            ("linux", "aarch64") => ("linux", "arm64"),
            ("linux", "x86") => ("linux", "386"),
            ("macos", "x86_64") => ("darwin", "x64"),
            ("macos", "aarch64") => ("darwin", "arm64"),
            _ => return, // no fixed expectation on other targets
        };
        let release = Release {
            tag: "v1.0.0".to_string(),
            assets: ["deb", "tar.gz", "zip"]
                .map(|ext| Asset {
                    url: format!("https://example.com/test-{os_kw}-{arch_kw}.{ext}"),
                })
                .to_vec(),
        };

        // Without filter: bpm cannot extract .deb; the first supported
        // format in list order (.tar.gz) must win.
        let selected = repo.select_asset(&release, false).unwrap();
        assert_eq!(
            selected,
            format!("https://example.com/test-{os_kw}-{arch_kw}.tar.gz")
        );

        // With filter: narrow to .zip only
        repo.asset_filter = vec![".zip".to_string()];
        let selected = repo.select_asset(&release, false).unwrap();
        assert_eq!(
            selected,
            format!("https://example.com/test-{os_kw}-{arch_kw}.zip")
        );
    }

    /// Real typstyle release assets (.exe mixed with .pdb debug symbols).
    /// Guards the best-first direction of `select_asset`: the regression once
    /// picked the .pdb (review P0-1).
    #[test]
    fn test_select_asset_typstyle_best_first() {
        let mut repo = Repo::new("typstyle");
        repo.repo_owner = Some("typst".to_string());
        repo.repo_name = Some("typstyle".to_string());

        let release = Release {
            tag: "v0.13.0".to_string(),
            assets: [
                "typstyle-alpine-x64",
                "typstyle-alpine-x64.debug",
                "typstyle-darwin-arm64",
                "typstyle-darwin-arm64.dwarf",
                "typstyle-darwin-x64",
                "typstyle-darwin-x64.dwarf",
                "typstyle-linux-arm64",
                "typstyle-linux-arm64.debug",
                "typstyle-linux-armhf",
                "typstyle-linux-armhf.debug",
                "typstyle-linux-x64",
                "typstyle-linux-x64.debug",
                "typstyle-win32-arm64.exe",
                "typstyle-win32-arm64.pdb",
                "typstyle-win32-x64.exe",
                "typstyle-win32-x64.pdb",
            ]
            .map(|name| Asset {
                url: format!("https://github.com/typst/typstyle/releases/download/v0.13.0/{name}"),
            })
            .to_vec(),
        };

        let selected = repo.select_asset(&release, false).unwrap();
        let expected = if cfg!(windows) {
            if cfg!(target_arch = "aarch64") {
                "typstyle-win32-arm64.exe"
            } else {
                "typstyle-win32-x64.exe"
            }
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "typstyle-darwin-arm64"
            } else {
                "typstyle-darwin-x64"
            }
        } else if cfg!(target_os = "linux") {
            if cfg!(target_arch = "aarch64") {
                "typstyle-linux-arm64"
            } else {
                "typstyle-linux-x64"
            }
        } else {
            return;
        };
        assert!(
            selected.ends_with(expected),
            "expected *{expected}, got {selected}"
        );
    }

    #[test]
    fn test_select_asset_empty_release() {
        let repo = Repo::new("test-pkg");
        let release = Release {
            tag: "v1.0.0".to_string(),
            assets: vec![],
        };
        let result = repo.select_asset(&release, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_asset_url_replacement() {
        // Test the URL string replacement logic used in the fast path
        let old_asset =
            "https://github.com/owner/repo/releases/download/v1.0.0/asset-linux-amd64.tar.gz";
        let old_version = "v1.0.0";
        let new_version = "v2.0.0";

        let candidate = old_asset.replace(old_version, new_version);
        assert_eq!(
            candidate,
            "https://github.com/owner/repo/releases/download/v2.0.0/asset-linux-amd64.tar.gz"
        );
        assert_ne!(candidate, old_asset);
    }

    #[test]
    fn test_github_release_deserialize_valid() {
        let raw = serde_json::json!({
            "tag_name": "v1.2.3",
            "prerelease": false,
            "assets": [
                {"browser_download_url": "https://example.com/asset1.zip"},
                {"browser_download_url": "https://example.com/asset2.tar.gz"},
            ]
        });
        let release = from_github_release(serde_json::from_value(raw).unwrap()).unwrap();
        assert_eq!(release.tag, "v1.2.3");
        assert_eq!(release.assets.len(), 2);
    }

    #[test]
    fn test_github_release_deserialize_no_assets() {
        let raw = serde_json::json!({ "tag_name": "v1.0.0", "assets": [] });
        let parsed: GithubRelease = serde_json::from_value(raw).unwrap();
        assert!(from_github_release(parsed).is_err());
    }

    #[test]
    fn test_github_search_result_deserialize() {
        let raw = serde_json::json!({
            "items": [ { "html_url": "https://github.com/owner/repo" } ]
        });
        let result: GithubSearchResult = serde_json::from_value(raw).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].html_url, "https://github.com/owner/repo");
    }
}
