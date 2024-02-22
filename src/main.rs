mod cli;
mod search;
mod utils;

use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use cli::Cli;
use colored::*;
use search::RepoHandler;
use std::sync::OnceLock;
use url::Url;

static cli: OnceLock<Cli> = OnceLock::new();

fn main() -> Result<()> {
    env_logger::init();
    // cli.set(Cli::parse());
    RepoHandler::new("eza".into()).ask(false).get_asset();
    Ok(())
}
