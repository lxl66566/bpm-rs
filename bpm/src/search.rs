use anyhow::Result;
use assert2::assert;
use colored::Colorize;
use die_exit::{die, Die};
use log::{debug, info};
use std::sync::LazyLock as Lazy;
use url::Url;

use crate::{
    storage::Repo,
    utils::{err::MyError, UrlJoinAll},
};

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
static REQUEST_CLIENT: Lazy<reqwest::blocking::Client> = Lazy::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent(APP_USER_AGENT)
        .build()
        .die("An error occured in building request client.")
});

/// A trait for searching
pub trait Searchable {
    fn search(&self) -> Result<Vec<String>>;
    fn ask(self, quiet: bool) -> Self;
    fn get_asset(&mut self) -> &mut Self;
    fn update_asset(&mut self) -> Option<(String, String)>;
}

impl Searchable for Repo {
    fn search(&self) -> Result<Vec<String>> {
        // Search API: https://docs.github.com/zh/rest/search/search?apiVersion=2022-11-28#search-repositories
        let url = Url::parse_with_params(
            self.site
                .api_base()
                .join_all_str(["search", "repositories"])?
                .as_str()
                .trim_matches('/'),
            &[
                ("q", format!("{} in:name", self.name).as_str()),
                ("page", "1"),
            ],
        )
        .expect("This construct should be ok.");
        debug!("search url: {}", &url);
        let response = REQUEST_CLIENT.get(url).send();
        match response {
            Ok(r) if r.status().is_success() => {
                let data: serde_json::Value = r.json().unwrap();
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
    fn ask(self, quiet: bool) -> Self {
        use terminal_menu::{button, label, menu, mut_menu, run};
        let items = self.search().die("An error occurs in searching repos.");
        assert!(!items.is_empty(), "No repos found.");
        if quiet {
            return self.set_by_url(items[0].as_str());
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
        self.set_by_url(selected)
    }

    fn get_asset(&mut self) -> &mut Self {
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
        match REQUEST_CLIENT.get(api).send() {
            Ok(response) if response.status().is_success() => {
                let releases: serde_json::Value = response
                    .json()
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
                    die!("{}", MyError::NoAvailableAsset);
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
    fn update_asset(&mut self) -> Option<(String, String)> {
        let old_version = self.version.clone().unwrap();
        self.get_asset();
        self.version.clone().and_then(|new_version| {
            if old_version == new_version {
                None
            } else {
                Some((old_version, new_version))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::Repo;
}
