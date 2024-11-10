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
use anyhow::Result;
use clap::Parser;
use cli::{Cli, SubCommand};

use utils::log_init;

#[tokio::main]
pub async fn main() -> Result<()> {
    log_init();
    let cli = Cli::parse();
    dbg!(&cli);

    Ok(())
}

pub fn install(cmd: SubCommand) -> Result<()> {
    Ok(())
}
