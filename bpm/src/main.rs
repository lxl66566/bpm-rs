#![warn(clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::expect_used,
    clippy::clone_on_ref_ptr,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::missing_docs_in_private_items
)]

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
