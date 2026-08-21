use std::{sync::LazyLock as Lazy, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
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
    /// Check for a newer version, try URL-replacement fast path, fall back to
    /// full asset selection.
    async fn update_asset(&mut self, interactive: bool) -> UpdateCheck;
}

/// Result of an update check.
#[derive(Debug)]
pub enum UpdateCheck {
    /// No newer release found: the current version equals the latest.
    UpToDate { current: String, latest: String },
    /// A newer release is available and ready to install.
    Update { old: String, new: String },
    /// The update check failed; carries the underlying error.
    Failed(anyhow::Error),
}

// -------- internal helpers (Repo) --------
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
            &[("per_page", per_page)],
        )?;

        debug!("Fetch releases from API: {api}");
        let releases: Vec<GithubRelease> = REQUEST_CLIENT
            .get(api)
            .send()
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

        // Sort preferred libc variant to the front
        let preferred_kw = self.libc_pref.keyword();
        let mut selected = architecture_select::sort_list(
            selected,
            &[(preferred_kw, architecture_select::MatchPos::All)],
            architecture_select::Combination::Any,
            false,
            false,
        );

        // Sort natively supported archive formats to the front, unsupported
        // formats (e.g. .deb, .rpm, .dmg, .AppImage) to the back so we pick
        // something bpm can actually extract.
        let supported_exts = [
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
        selected = architecture_select::sort_list(
            selected,
            &supported_exts,
            architecture_select::Combination::Any,
            false,
            false,
        );

        if interactive && selected.len() > 1 {
            ask_asset_interactive(&selected)
        } else {
            selected.pop().context(format!(
                "No valid asset found for '{}'. Try --interactive.",
                self.name
            ))
        }
    }
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

async fn check_url_exists(url: &str) -> bool {
    // Try HEAD first (lightweight), fall back to ranged GET for CDNs/proxies
    // that don't support HEAD.
    if REQUEST_CLIENT
        .head(url)
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
    {
        return true;
    }
    REQUEST_CLIENT
        .get(url)
        .header("Range", "bytes=0-0")
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
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
            &[
                ("q", format!("{} in:name", self.name).as_str()),
                ("page", "1"),
                ("sort", sort.as_ref()),
            ],
        )?;

        debug!("search url: {url}");
        let response = REQUEST_CLIENT
            .get(url)
            .send()
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

    async fn update_asset(&mut self, interactive: bool) -> UpdateCheck {
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
        let candidate = old_asset.replace(&old_version, &release.tag);
        if candidate != old_asset && check_url_exists(&candidate).await {
            self.version = Some(release.tag.clone());
            self.asset = Some(candidate);
            return UpdateCheck::Update {
                old: old_version,
                new: release.tag,
            };
        }

        // Fallback: reuse the already-fetched release for full asset selection
        let asset = match self.select_asset(&release, interactive) {
            Ok(asset) => asset,
            Err(e) => return UpdateCheck::Failed(e),
        };
        self.version = Some(release.tag.clone());
        self.asset = Some(asset);
        UpdateCheck::Update {
            old: old_version,
            new: release.tag,
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

    #[test]
    fn test_select_asset_with_filter() {
        let mut repo = Repo::new("test-pkg");
        repo.repo_owner = Some("owner".to_string());
        repo.repo_name = Some("repo".to_string());

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            let release = Release {
                tag: "v1.0.0".to_string(),
                assets: vec![
                    Asset {
                        url: "https://example.com/test-linux-amd64.deb".to_string(),
                    },
                    Asset {
                        url: "https://example.com/test-linux-amd64.tar.gz".to_string(),
                    },
                    Asset {
                        url: "https://example.com/test-linux-amd64.zip".to_string(),
                    },
                ],
            };

            // Without filter: should pick a supported format
            let selected = repo.select_asset(&release, false).unwrap();
            assert!(selected.ends_with(".tar.gz") || selected.ends_with(".zip"));

            // With filter: should narrow to .zip only
            repo.asset_filter = vec![".zip".to_string()];
            let selected = repo.select_asset(&release, false).unwrap();
            assert!(selected.ends_with(".zip"));
        }
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
