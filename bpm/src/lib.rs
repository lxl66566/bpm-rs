#![warn(clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::expect_used,
    clippy::clone_on_ref_ptr,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::missing_docs_in_private_items,
    clippy::struct_field_names,
    clippy::module_name_repetitions
)]

mod cli;
mod config;
mod installation;
mod search;
mod storage;
pub mod utils;

use std::{path::PathBuf, sync::LazyLock as Lazy};

use anyhow::Result;
use clap::Parser;
use cli::Cli;

static CLI: Lazy<Cli> = Lazy::new(Cli::parse);

// #[tokio::main]
// async fn main() -> Result<()> {
//     env_logger::init();
//     let mut repo = Repo::new("eza");
//     let res = repo.search().await.expect("search repo failed.");
//     repo.ask(res, false);
//     repo.get_asset().await;
//     println!("{repo}");
//     Ok(())
// }

use trauma::{download::Download, downloader::DownloaderBuilder};

#[tokio::main]
pub async fn main() -> Result<()> {
    let reqwest_rs = "https://github.com/seanmonstar/reqwest/archive/refs/tags/v0.11.9.zip";
    let downloads = vec![
        Download::try_from(reqwest_rs).unwrap(),
        Download::try_from(reqwest_rs).unwrap(),
    ];
    let downloader = DownloaderBuilder::new()
        .directory(PathBuf::from("output"))
        .build();
    downloader.download(&downloads).await;
    Ok(())
}
