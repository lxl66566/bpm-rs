mod cli;
mod search;
mod utils;

use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use cli::Cli;
use colored::*;
use once_cell::sync::Lazy;
use search::RepoHandler;
use url::Url;

static CLI: Lazy<Cli> = Lazy::new(|| Cli::parse());

fn main() -> Result<()> {
    env_logger::init();
    RepoHandler::new("eza".into()).ask(false).get_asset();
    Ok(())
}
