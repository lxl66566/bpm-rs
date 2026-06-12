mod log;

use anyhow::Result;
use bin_package_manager::{cli::Cli, context::Context, dispatch};
use clap::Parser;
use log::log_init;

#[tokio::main]
async fn main() -> Result<()> {
    log_init();
    let cli = Cli::parse();
    let ctx = Context::new();
    dispatch(cli, ctx).await
}
