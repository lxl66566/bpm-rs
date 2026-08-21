mod log;

use anyhow::Result;
use bin_package_manager::{cli::Cli, config::Config, context::Context, dispatch};
use clap::Parser;
use log::log_init;

#[tokio::main]
async fn main() -> Result<()> {
    log_init();
    let cli = Cli::parse();
    // Set the active config path before any API call (the HTTP client reads
    // the token from it lazily).
    let config_path = Config::resolve_path(cli.config.as_deref())?;
    Config::set_active_path(config_path.clone());
    let config = Config::load(&config_path)?;
    let ctx = Context::from_config(&config, cli.prefix.clone());
    dispatch(cli, ctx).await
}
