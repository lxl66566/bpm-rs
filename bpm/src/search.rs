use std::sync::LazyLock as Lazy;

use anyhow::Result;
use assert2::assert;
use colored::Colorize;
use die_exit::{die, Die};
use log::{debug, info};
use tokio::sync::mpsc;
use url::Url;

use crate::{
    cli::SortParam,
    storage::{Repo, RepoList},
    utils::UrlJoinAll,
};

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
static REQUEST_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .build()
        .die("An error occured in building request client.")
});

/// A trait for searching
pub trait Searchable {
    async fn search(&self, sort: SortParam) -> Result<Vec<String>>;
    fn ask(&mut self, items: Vec<String>, quiet: bool);
    async fn get_asset(&mut self) -> &mut Self;
    async fn update_asset(&mut self) -> Option<(String, String)>;
}

impl Searchable for Repo {
    async fn search(&self, sort: SortParam) -> Result<Vec<String>> {
        // Search API: https://docs.github.com/zh/rest/search/search?apiVersion=2022-11-28#search-repositories
        if self.url().is_some() {
            debug!(
                "Repo `{}`'s url already exists. Skipping search.",
                self.name
            );
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
        )
        .expect("This construct should be ok.");
        debug!("search url: {}", &url);
        let response = REQUEST_CLIENT.get(url).send().await;
        match response {
            Ok(r) if r.status().is_success() => {
                let data: serde_json::Value = r.json().await.unwrap();
                data["items"].as_array().map_or_else(
                    || {
                        die!("No items found in the response");
                    },
                    |items| {
                        let repos: Vec<String> = items
                            .iter()
                            .map(|item| item["html_url"].as_str().unwrap_or_default().to_string())
                            .collect();
                        Ok(repos)
                    },
                )
            }
            Ok(r) => {
                die!("Unexpected status: {}", r.status());
            }
            Err(e) => {
                die!("Error fetching data: {}", e);
            }
        }
    }

    #[allow(clippy::significant_drop_tightening)]
    fn ask(&mut self, items: Vec<String>, quiet: bool) {
        use terminal_menu::{button, label, menu, mut_menu, run};
        assert!(!items.is_empty(), "No repos found.");
        if quiet {
            self.set_by_url(items[0].as_str());
            return;
        }
        let mut menu_items = vec![label(
            "Please select the repo you want to install:"
                .bold()
                .to_string(),
        )];
        menu_items.reserve(items.len());
        items
            .into_iter()
            .map(button)
            .for_each(|x| menu_items.push(x));
        let select_menu = menu(menu_items);
        run(&select_menu);
        let temp = mut_menu(&select_menu);
        let selected = temp.selected_item_name();
        info!("selected repo: {}", selected);
        self.set_by_url(selected);
    }

    async fn get_asset(&mut self) -> &mut Self {
        assert!(self.repo_owner.is_some() && self.repo_name.is_some());
        let api = self
            .site
            .api_base()
            .join_all_str([
                "repos",
                self.repo_owner.as_deref().unwrap(),
                self.repo_name.as_deref().unwrap(),
                "releases",
                "latest",
            ])
            .expect("Invalid path.");
        debug!("Get assets from API: {}", api);
        match REQUEST_CLIENT.get(api).send().await {
            Ok(response) if response.status().is_success() => {
                let releases: serde_json::Value = response
                    .json()
                    .await
                    .die("Assets API response is not a valid json");

                self.version = Some(
                    releases["tag_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                );

                let raw_assets = releases["assets"]
                    .as_array()
                    .die("Assets API response has no array named `assets`");
                if raw_assets.is_empty() {
                    die!(
                        "No releases found for {}/{}",
                        self.repo_owner.as_ref().unwrap(),
                        self.repo_name.as_ref().unwrap()
                    );
                }

                let assets: Vec<String> = raw_assets
                    .iter()
                    .filter_map(|asset| asset["browser_download_url"].as_str().map(String::from))
                    .collect();

                let assets = architecture_select::select(assets);

                if let Some(selected_asset) = assets.first() {
                    self.asset = Some(selected_asset.to_string());
                    eprintln!("Selected asset: {selected_asset}");
                    self
                } else {
                    die!("No available asset found in this repo. If you're sure there's a valid asset, use `--interactive`.");
                }
            }
            Ok(response) => {
                die!(
                    "Unexpected response status: {} from the releases API",
                    response.status()
                );
            }
            Err(err) => {
                die!("Error fetching releases data: {}", err);
            }
        }
    }

    ///  update assets list. Returns `None` if has no update, `(old_version,
    /// new_version)` if has update.
    async fn update_asset(&mut self) -> Option<(String, String)> {
        let old_version = self.version.clone().unwrap();
        self.get_asset().await;
        self.version.clone().and_then(|new_version| {
            if old_version == new_version {
                None
            } else {
                Some((old_version, new_version))
            }
        })
    }
}

pub trait SearchableSequence {
    async fn pre_install(self, quiet: bool, interactive: bool, sort: SortParam) -> Self;
}

impl SearchableSequence for RepoList {
    async fn pre_install(self, quiet: bool, interactive: bool, sort: SortParam) -> Self {
        let (tx, rx) = mpsc::channel(self.len());

        for repo in self.0 {
            let tx = tx.clone();
            tokio::spawn(async move {
                let search_result = repo.search(sort).await;
                tx.send((repo, search_result)).await
            });
        }

        todo!()
    }
}

#[cfg(test)]
mod tests {}
