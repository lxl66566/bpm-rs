use std::sync::LazyLock as Lazy;

use anyhow::{Result, anyhow, bail};
use colored::Colorize;
use log::{debug, info};
use reqwest::StatusCode;
use url::Url;

use crate::{
    cli::SortParam,
    error::BpmError,
    storage::{Repo, RepoList},
    utils::UrlJoinAll,
};

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
static REQUEST_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .build()
        .expect("Failed to build HTTP client")
});

pub trait Searchable {
    async fn search(&self, sort: SortParam) -> Result<Vec<String>>;
    fn ask(&mut self, items: Vec<String>, quiet: bool) -> Result<()>;
    async fn get_asset(&mut self, interactive: bool) -> Result<()>;
    async fn update_asset(&mut self) -> Option<(String, String)>;
}

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

        let data: serde_json::Value = response.json().await?;

        let items = data["items"]
            .as_array()
            .ok_or_else(|| anyhow!("No items found in search response"))?;

        let repos: Vec<String> = items
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
            self.set_by_url(items[0].as_str());
            return Ok(());
        }

        use terminal_menu::{button, label, menu, mut_menu, run};
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
        let selected = temp.selected_item_name();
        info!("selected repo: {selected}");
        self.set_by_url(selected);
        Ok(())
    }

    async fn get_asset(&mut self, interactive: bool) -> Result<()> {
        let (owner, name) = (self.repo_owner.as_deref(), self.repo_name.as_deref());
        if owner.is_none() || name.is_none() {
            bail!("repo_owner or repo_name not set");
        }

        let api = self.site.api_base().join_all_str([
            "repos",
            owner.unwrap(),
            name.unwrap(),
            "releases",
            "latest",
        ])?;

        debug!("Get assets from API: {api}");
        let response = REQUEST_CLIENT.get(api).send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            bail!("No releases found for {}/{}", owner.unwrap(), name.unwrap());
        }
        if !response.status().is_success() {
            bail!("GitHub releases API returned status: {}", response.status());
        }

        let releases: serde_json::Value = response.json().await?;
        self.version = releases["tag_name"].as_str().map(String::from);

        let raw_assets = releases["assets"].as_array().ok_or_else(|| {
            BpmError::AssetNotFound(owner.unwrap().to_string(), name.unwrap().to_string())
        })?;

        if raw_assets.is_empty() {
            return Err(BpmError::AssetNotFound(
                owner.unwrap().to_string(),
                name.unwrap().to_string(),
            )
            .into());
        }

        let assets: Vec<String> = raw_assets
            .iter()
            .filter_map(|a| a["browser_download_url"].as_str().map(String::from))
            .collect();

        let filtered = if !self.asset_filter.is_empty() {
            assets
                .into_iter()
                .filter(|a| self.asset_filter.iter().all(|f| a.contains(f)))
                .collect::<Vec<_>>()
        } else {
            assets
        };

        let selected = architecture_select::select(filtered);

        if interactive && selected.len() > 1 {
            let choice = ask_asset_interactive(&selected)?;
            self.asset = Some(choice);
        } else if let Some(asset) = selected.first() {
            self.asset = Some(asset.clone());
        } else {
            return Err(BpmError::InvalidAsset(self.name.clone()).into());
        }

        info!("Selected asset: {}", self.asset.as_deref().unwrap_or(""));
        Ok(())
    }

    async fn update_asset(&mut self) -> Option<(String, String)> {
        let old_version = self.version.clone()?;
        if self.get_asset(false).await.is_err() {
            return None;
        }
        let new_version = self.version.clone()?;
        if old_version == new_version {
            None
        } else {
            Some((old_version, new_version))
        }
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
    let selected_name = binding.selected_item_name();
    assets
        .iter()
        .find(|a| a.rsplit('/').next().unwrap_or(a) == selected_name)
        .cloned()
        .ok_or_else(|| anyhow!("Selected asset not found"))
}

pub trait SearchableSequence {
    async fn search_all(self, quiet: bool, interactive: bool, sort: SortParam) -> Result<RepoList>;
}

impl SearchableSequence for RepoList {
    async fn search_all(self, quiet: bool, interactive: bool, sort: SortParam) -> Result<RepoList> {
        let mut results = Vec::new();
        for mut repo in self.0 {
            if repo.url().is_none() {
                let items = repo.search(sort).await?;
                if !items.is_empty() {
                    repo.ask(items, quiet)?;
                }
            }
            if !interactive || repo.asset.is_none() {
                repo.get_asset(interactive).await?;
            }
            results.push(repo);
        }
        Ok(RepoList(results))
    }
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
