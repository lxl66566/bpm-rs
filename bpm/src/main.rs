#![warn(clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::expect_used,
    clippy::clone_on_ref_ptr,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::missing_docs_in_private_items,
    clippy::struct_field_names
)]

mod cli;
mod config;
mod search;
mod storage;
mod utils;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use search::Searchable;
use std::sync::LazyLock as Lazy;
use storage::Repo;

static CLI: Lazy<Cli> = Lazy::new(Cli::parse);

fn main() -> Result<()> {
    env_logger::init();
    Repo::new("eza").ask(false).get_asset();
    Ok(())
}
