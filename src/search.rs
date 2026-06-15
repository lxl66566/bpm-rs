use std::sync::LazyLock as Lazy;

use anyhow::{Context, Result, anyhow, bail};
use colored::Colorize;
use log::{debug, info};
use terminal_menu::{button, label, menu, mut_menu, run};
use url::Url;

use crate::{cli::SortParam, storage::Repo, utils::UrlJoinAll};

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
static REQUEST_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    let mut headers = reqwest::header::HeaderMap::new();

    // GitHub API version header
    headers.insert("X-GitHub-Api-Version", reqwest::header::HeaderValue::from_static("2022-11-28"));

    // Read token from GITHUB_TOKEN or GH_TOKEN environment variable
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN"))
        && let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(reqwest::header::AUTHORIZATION, value);
        debug!("Using GitHub token from environment for higher API rate limit");
    }

    reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
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
    async fn update_asset(&mut self, interactive: bool) -> Option<(String, String)>;
}

// -------- internal helpers (Repo) --------
impl Repo {
    /// Fetch the latest release from the platform API and parse into a typed
    /// [`Release`]. Platform-specific parsing is isolated here.
    async fn fetch_latest_release(&self) -> Result<Release> {
        let Some(owner) = self.repo_owner.as_deref() else {
            bail!("repo_owner not set");
        };
        let Some(name) = self.repo_name.as_deref() else {
            bail!("repo_name not set");
        };

        let per_page = if self.allow_pre { "1" } else { "100" };
        let api = Url::parse_with_params(
            self.site
                .api_base()
                .join_all_str(["repos", owner, name, "releases"])?
                .as_str(),
            &[("per_page", per_page)],
        )?;

        debug!("Fetch releases from API: {api}");
        let response = REQUEST_CLIENT.get(api).send().await?;
        if !response.status().is_success() {
            bail!("GitHub releases API returned status: {}", response.status());
        }

        let data: serde_json::Value = response.json().await?;
        let raw_releases = data
            .as_array()
            .ok_or_else(|| anyhow!("Expected array from releases endpoint"))?;

        let raw = if self.allow_pre {
            raw_releases.first()
        } else {
            raw_releases
                .iter()
                .find(|r| r["prerelease"].as_bool() == Some(false))
        };
        let raw = raw.ok_or_else(|| {
            anyhow!("No release asset found for {owner}/{name}. The repo may have no releases.")
        })?;

        parse_github_release(raw, owner, name)
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
            selected
                .pop()
                .context(format!("No valid asset found for '{}'. Try --interactive.", self.name))
        }
    }
}

/// Isolate GitHub JSON → Release conversion here.
/// When adding GitLab etc., add a sister function.
fn parse_github_release(raw: &serde_json::Value, owner: &str, name: &str) -> Result<Release> {
    let tag = raw["tag_name"]
        .as_str()
        .with_context(|| format!("Release for {owner}/{name} has no tag_name"))?
        .to_string();

    let assets = raw["assets"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a["browser_download_url"].as_str())
                .map(|u| Asset { url: u.to_string() })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if assets.is_empty() {
        bail!("No downloadable assets in release for {owner}/{name}");
    }

    Ok(Release { tag, assets })
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
        let response = REQUEST_CLIENT.get(url).send().await?;

        if !response.status().is_success() {
            bail!("GitHub search API returned status: {}", response.status());
        }

        let repos: Vec<String> = response.json::<serde_json::Value>().await?["items"]
            .as_array()
            .ok_or_else(|| anyhow!("No items found in search response"))?
            .iter()
            .filter_map(|item| item["html_url"].as_str().map(String::from))
            .collect();

        Ok(repos)
    }

    fn ask(&mut self, items: Vec<String>, quiet: bool) -> Result<()> {
        if items.is_empty() {
            bail!("No repos found for '{}'", self.name);
        }

        if quiet {
            self.set_by_url(&items[0])?;
            return Ok(());
        }

        let mut menu_items = vec![label(
            "Please select the repo you want to install:"
                .bold()
                .to_string(),
        )];
        for item in &items {
            menu_items.push(button(item));
        }
        let select_menu = menu(menu_items);
        run(&select_menu);
        let temp = mut_menu(&select_menu);
        if temp.canceled() {
            bail!("User cancelled the repo selection");
        }
        let selected = temp.selected_item_name();
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

    async fn update_asset(&mut self, interactive: bool) -> Option<(String, String)> {
        let old_version = self.version.clone()?;
        let old_asset = self.asset.clone()?;

        let release = self.fetch_latest_release().await.ok()?;

        if old_version == release.tag {
            return None;
        }

        // Fast path: replace old version string with new one in the asset URL
        let candidate = old_asset.replace(&old_version, &release.tag);
        if candidate != old_asset && check_url_exists(&candidate).await {
            self.version = Some(release.tag.clone());
            self.asset = Some(candidate);
            return Some((old_version, release.tag));
        }

        // Fallback: reuse the already-fetched release for full asset selection
        let asset = self.select_asset(&release, interactive).ok()?;
        self.version = Some(release.tag.clone());
        self.asset = Some(asset);
        Some((old_version, release.tag))
    }
}

fn ask_asset_interactive(assets: &[String]) -> Result<String> {
    use terminal_menu::{button, label, menu, mut_menu, run};
    let mut items = vec![label("Select an asset:".bold().to_string())];
    for asset in assets {
        let short = asset.rsplit('/').next().unwrap_or(asset);
        items.push(button(short));
    }
    let m = menu(items);
    run(&m);
    let binding = mut_menu(&m);
    if binding.canceled() {
        bail!("User cancelled the asset selection");
    }
    let selected_name = binding.selected_item_name();
    assets
        .iter()
        .find(|a| a.rsplit('/').next().unwrap_or(a) == selected_name)
        .cloned()
        .ok_or_else(|| anyhow!("Selected asset not found"))
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
}
