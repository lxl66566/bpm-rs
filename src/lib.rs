#![warn(clippy::cargo)]
#![allow(
    clippy::clone_on_ref_ptr,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::missing_docs_in_private_items,
    clippy::struct_field_names,
    clippy::module_name_repetitions
)]

mod cli;
mod command;
pub mod context;
pub mod error;
pub mod installation;
mod search;
pub mod storage;
pub mod utils;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use context::Context;
use utils::log_init;

pub async fn run() -> Result<()> {
    log_init();
    let cli = Cli::parse();
    let ctx = Context::new();
    command::dispatch(cli, ctx).await
}
