use crate::utils::constants::OPTION_REPO_NUM;
use crate::utils::err::MyError;
use crate::utils::{fmt_repo_list, path_join, UrlJoinAll};
use anyhow::Result;
use assert2::{assert, check};
use die_exit::{die, Die, DieWith};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::env::consts::{ARCH, OS};
use std::fmt::{self, format};
use std::path::{Path, PathBuf};
use url::Url;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[non_exhaustive]
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct RepoHandler {
    name: String,
    bin_name: String,
    site: &'static str,
    repo_name: Option<String>,
    repo_owner: Option<String>,
    asset: Option<String>,
    version: Option<String>,
    installed_files: Vec<PathBuf>,
    prefer_gnu: bool,
    no_pre: bool,
    one_bin: bool,
}

impl RepoHandler {
    pub fn new(name: String) -> Self {
        #[cfg(windows)]
        assert!(
            !["app", "bin"].contains(&name.as_str()),
            "Invalid repo name: `{}`. Must not be one of them: `app`, `bin`",
            name
        );
        RepoHandler {
            name: name.clone(),
            #[cfg(not(windows))]
            bin_name: name.clone(),
            #[cfg(windows)]
            bin_name: "*.exe".into(),
            site: "github",
            repo_name: None,
            repo_owner: None,
            asset: None,
            version: None,
            installed_files: Vec::new(),
            prefer_gnu: false,
            no_pre: false,
            one_bin: false,
        }
    }

    pub fn with_bin_name(mut self, bin_name: String) -> Self {
        #[cfg(windows)]
        {
            self.bin_name = if bin_name.ends_with(".exe") {
                bin_name
            } else {
                bin_name + ".exe"
            };
        }
        #[cfg(not(windows))]
        {
            self.bin_name = bin_name;
        }
        self
    }

    pub fn url(&self) -> Url {
        check!(
            self.repo_name.is_some() || self.repo_owner.is_some(),
            "repo_name and repo_owner must be set"
        );
        self.base()
            .join_all_str([
                self.repo_owner.as_deref().unwrap(),
                self.repo_name.as_deref().unwrap(),
            ])
            .die_with(|e| format!("trying to construct an invalid url. Err: {e}"))
    }

    /// use Github as default.
    pub fn base(&self) -> Url {
        Url::parse("https://github.com").expect("hardcoded URL should be valid")
    }

    /// use Github as default.
    pub fn api_base(&self) -> Url {
        Url::parse("https://api.github.com").expect("hardcoded URL should be valid")
    }

    pub fn dedup_file_list(&mut self) {
        self.installed_files.sort();
        self.installed_files.dedup();
        debug!("dedup file list success: {:#?}", self.installed_files);
    }

    pub fn add_file_list(&mut self, file: PathBuf) {
        self.installed_files.push(file.clone());
        debug!("added file `{}` to file_list", file.display());
    }

    /// Set the `repo_name` and `repo_owner` by fullname.
    /// For example, with the full name `me/myrepo`, the `repo_owner` would be
    /// `me`, and the `repo_name` would be `myrepo`.
    #[allow(clippy::unwrap_used)]
    pub fn set_by_fullname(mut self, full_name: &str) -> Self {
        let mut iter = full_name.trim_matches('/').split('/');
        self.repo_owner = Some(
            iter.next()
                .unwrap_or_else(|| die!("An error occurs in parsing full name 1st part"))
                .to_string(),
        );
        self.repo_name = Some(
            iter.next()
                .unwrap_or_else(|| die!("An error occurs in parsing full name 2nd part"))
                .to_string(),
        );
        debug_assert!(iter.next().is_none(), "fullname has more than 2 parts");
        debug!(
            "set repo_name: {}, repo_owner: {}",
            self.repo_name.as_ref().unwrap(),
            self.repo_owner.as_ref().unwrap()
        );
        self
    }
    /// Set the `repo_name` and `repo_owner` by url.
    /// For example, with the url `https://github.com/lxl66566/bpm-rs/`, the `repo_owner` would be
    /// `lxl66566`, and the `repo_name` would be `bpm-rs`.
    pub fn set_by_url(self, url: &str) -> Self {
        let binding = Url::parse(url).expect("parsing invalid URL.");
        let full_name = binding.path();
        self.set_by_fullname(full_name)
    }

    fn search(&self, page: u32) -> Result<Vec<String>> {
        // Search API: https://docs.github.com/zh/rest/search/search?apiVersion=2022-11-28#search-repositories
        let url = Url::parse_with_params(
            self.api_base()
                .join_all_str(["search", "repositories"])?
                .as_str(),
            &[
                ("q", format!("{} in:name", self.name).as_str()),
                ("page", page.to_string().as_str()),
                ("per_page", &OPTION_REPO_NUM.to_string()),
            ],
        )
        .expect("This construct should be ok.");
        info!("searching url: {}", &url);
        let client = reqwest::blocking::Client::builder()
            .user_agent(APP_USER_AGENT)
            .build()?;
        let response = client.get(url).send();
        match response {
            Ok(r) if r.status().is_success() => {
                let data: serde_json::Value = r.json().unwrap();
                trace!("search response: {:?}", &data);
                if let Some(items) = data.as_array() {
                    let repos: Vec<String> = items
                        .iter()
                        .map(|item| item["html_url"].as_str().unwrap_or_default().to_string())
                        .collect();
                    Ok(repos)
                } else {
                    die!("No items found in the response");
                }
            }
            Ok(r) => {
                die!("Unexpected status code: {}", r.status());
            }
            Err(e) => {
                die!("Error fetching data: {}", e);
            }
        }
    }

    pub fn ask(self, quiet: bool) -> Self {
        let mut page = 1;
        loop {
            if let Ok(repo_selections) = self.search(page) {
                if repo_selections.is_empty() {
                    println!("No repos found in this page");
                    continue;
                }
                if quiet {
                    eprintln!("auto select repo: {}", repo_selections[0]);
                    return self.set_by_url(&repo_selections[0]);
                }
                for (i, item) in repo_selections.iter().enumerate() {
                    println!("{}: {}", i + 1, item);
                }
                let temp = match self.read_input("please select a repo to download (default 1), `m` for more, `p` for previous: ") {
                    Ok(val) => val.trim().to_string(),
                    Err(_) => die!("Invalid input")
                };

                let index = if temp.is_empty() {
                    1
                } else {
                    match temp.parse::<usize>() {
                        Ok(val) => val,
                        Err(_) => {
                            match temp.as_str() {
                                "m" => page += 1,
                                "p" => page = page.saturating_sub(1),
                                _ => eprintln!("Invalid input"),
                            }
                            continue;
                        }
                    }
                };

                match index {
                    1..=OPTION_REPO_NUM => return self.set_by_url(&repo_selections[index - 1]),
                    _ => eprintln!(
                        "Invalid input: the number should not be more than {}",
                        OPTION_REPO_NUM
                    ),
                }
            } else {
                die!("An error occured in constructing url in searching.");
            }
        }
    }

    pub fn get_asset(&mut self) -> Option<&mut Self> {
        debug_assert!(self.repo_owner.is_some() && self.repo_name.is_some());
        let api = self
            .api_base()
            .join_all_str([
                "repos",
                self.repo_owner.as_deref().unwrap(),
                self.repo_name.as_deref().unwrap(),
                "latest",
            ])
            .expect("Invalid path.");
        info!("Get assets from API: {}", api);
        match reqwest::blocking::get(api) {
            Ok(response) if response.status().is_success() => {
                let releases: Vec<serde_json::Value> = response.json().unwrap();
                if releases.is_empty() {
                    die!(
                        "No releases found for {}/{}",
                        self.repo_owner.as_ref().unwrap(),
                        self.repo_name.as_ref().unwrap()
                    );
                }
                let release = &releases[0];
                self.version = Some(release["tag_name"].as_str().unwrap_or_default().to_string());
                let raw_assets = release["assets"].as_array().unwrap();

                let mut assets: Vec<String> = raw_assets
                    .iter()
                    .filter_map(|asset| asset["browser_download_url"].as_str().map(String::from))
                    .collect();

                fn not_empty_filter(
                    vec: Vec<String>,
                    filter: impl Fn(&String) -> bool,
                ) -> Vec<String> {
                    let temp: Vec<String> = vec.clone().into_iter().filter(filter).collect();
                    if temp.is_empty() {
                        vec
                    } else {
                        temp
                    }
                }

                // Select platform
                assets = not_empty_filter(assets, |asset| asset.to_lowercase().contains(OS));

                #[cfg(windows)]
                if !self.name.to_lowercase().contains("win") {
                    assets.retain(|asset| asset.to_lowercase().contains("win"));
                    if assets.is_empty() {
                        die!(MyError::NoAvailableAsset);
                    }
                }

                // Select architecture
                assets = not_empty_filter(assets, |asset| asset.to_lowercase().contains(ARCH));

                // Prefer GNU
                if !self.prefer_gnu {
                    assets.sort_by(|a, b| {
                        a.to_lowercase()
                            .contains("musl")
                            .cmp(&b.to_lowercase().contains("musl"))
                    });
                }

                // Sort by archive type
                assets.sort_by_key(|a| a.ends_with(".7z"));
                // further sort by archive format
                #[cfg(windows)]
                {
                    assets.sort_by_key(|a| a.contains(".tar."));
                    assets.sort_by_key(|a| a.ends_with(".zip"));
                }
                #[cfg(not(windows))]
                {
                    assets.sort_by_key(|a| a.ends_with(".zip"));
                    assets.sort_by_key(|a| a.contains(".tar."));
                }

                if let Some(selected_asset) = assets.first() {
                    self.asset = Some(selected_asset.to_string());
                    eprintln!("Selected asset: {}", selected_asset);
                    Some(self)
                } else {
                    die!(MyError::NoAvailableAsset);
                }
            }
            Ok(response) => {
                die!(
                    "Unexpected response status:{} from the releases API",
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
    pub fn update_asset(&mut self) -> Option<(String, String)> {
        let old_version = self.version.clone().unwrap();
        if self.get_asset().is_some() {
            if let Some(new_version) = self.version.clone() {
                if &old_version == &new_version {
                    None
                } else {
                    Some((old_version, new_version))
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn read_input(&self, prompt: &str) -> Result<String, std::io::Error> {
        eprint!("{}", prompt);
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        Ok(input.trim().to_string())
    }
}

impl fmt::Display for RepoHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            fmt_repo_list(
                self.name.as_str(),
                self.url().as_str(),
                self.version.as_deref().unwrap_or_default()
            )
        )
    }
}

impl PartialEq for RepoHandler {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for RepoHandler {}

impl Ord for RepoHandler {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialOrd for RepoHandler {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_by_url() {
        let repo = RepoHandler::default().set_by_url("https://github.com/lxl66566/bpm-rs/");
        assert_eq!(
            repo.clone().url().as_str().trim_matches('/'),
            "https://github.com/lxl66566/bpm-rs"
        );
        assert_eq!(repo.repo_name.unwrap(), "bpm-rs");
        assert_eq!(repo.repo_owner.unwrap(), "lxl66566");
    }
}
