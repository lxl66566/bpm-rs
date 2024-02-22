use crate::utils::constants::OPTION_REPO_NUM;
use crate::utils::err::{self, invalid_asset_error};
use crate::utils::{assert_exit, error_exit, fmt_repo_list, path_join};
use anyhow::Result;
use assert2::check;
use colored::Colorize;
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::env::consts::{ARCH, OS};
use std::path::{Path, PathBuf};
use std::vec::Vec;
use std::{fmt, path};
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
        assert_exit!(
            !["app", "bin"].contains(&name.as_str()),
            "Invalid repo name: `{}`. Must not be one of: app, bin",
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
        self.api_base()
            .join(
                Path::new(self.repo_owner.as_deref().unwrap())
                    .join(self.repo_name.as_deref().unwrap())
                    .to_str()
                    .expect("url parse error"),
            )
            .expect("url parse error")
    }

    pub fn api_base(&self) -> Url {
        match self.site {
            "github" => Url::parse("https://api.github.com"),
            _ => unimplemented!(),
        }
        .expect("hardcoded URL should be valid")
    }

    pub fn file_list(&mut self) -> &Vec<PathBuf> {
        self.installed_files.sort();
        self.installed_files.dedup();
        &self.installed_files
    }

    pub fn file_list_mut(&mut self) -> &mut Vec<PathBuf> {
        self.installed_files.sort();
        self.installed_files.dedup();
        &mut self.installed_files
    }

    pub fn add_file_list(&mut self, file: PathBuf) {
        self.installed_files.push(file);
    }

    pub fn set_by_fullname(mut self, full_name: &str) -> Self {
        let mut iter = full_name.split('/');
        self.repo_name = Some(
            iter.next()
                .unwrap_or_else(|| error_exit!("An error occurs in parsing full name 1st part"))
                .to_string(),
        );
        self.repo_owner = Some(
            iter.next()
                .unwrap_or_else(|| error_exit!("An error occurs in parsing full name 2nd part"))
                .to_string(),
        );
        debug_assert!(iter.next().is_none(), "fullname has more than 2 parts");
        self
    }

    pub fn set_by_url(self, url: &str) -> Self {
        let binding = Url::parse(url).expect("parsing invalid URL.");
        let full_name = binding.path().trim_matches('/');
        self.set_by_fullname(full_name)
    }

    fn search(&self, page: u32) -> Result<Vec<String>> {
        let url = Url::parse_with_params(
            self.api_base()
                .join("search")?
                .join("repositories")?
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
                    error_exit!("No items found in the response");
                }
            }
            Ok(r) => {
                error_exit!("Unexpected status code: {}", r.status());
            }
            Err(e) => {
                error_exit!("Error fetching data: {}", e);
            }
        }
    }

    pub fn ask(mut self, quiet: bool) -> Self {
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
                    Err(_) => error_exit!("Invalid input")
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
                error_exit!("An error occured in constructing url in searching.");
            }
        }
    }

    pub fn get_asset(&mut self) -> Option<&mut Self> {
        debug_assert!(self.repo_owner.is_some() && self.repo_name.is_some());
        let api = self
            .api_base()
            .join(
                path_join([
                    "repos",
                    self.repo_owner.as_deref().unwrap(),
                    self.repo_name.as_deref().unwrap(),
                    "latest",
                ])
                .to_str()
                .expect("Invalid path."),
            )
            .expect("Invalid path.");
        match reqwest::blocking::get(api) {
            Ok(response) if response.status().is_success() => {
                let releases: Vec<serde_json::Value> = response.json().unwrap();
                if releases.is_empty() {
                    error_exit!(
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
                        invalid_asset_error();
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
                    invalid_asset_error()
                }
            }
            Ok(response) => {
                error_exit!(
                    "Unexpected response status:{} from the releases API",
                    response.status()
                );
            }
            Err(err) => {
                error_exit!("Error fetching releases data: {}", err);
            }
        }
    }

    ///  update assets list. Returns `None` if has no update, `(old_version, new_version)` if has update.
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
