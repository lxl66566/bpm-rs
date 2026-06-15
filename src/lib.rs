#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_docs_in_private_items,
    clippy::missing_panics_doc,
    clippy::missing_safety_doc,
    clippy::missing_errors_doc,
    clippy::assigning_clones,
    clippy::fn_params_excessive_bools,
    clippy::too_many_lines
)]

pub mod cli;
pub mod command;
pub mod context;
pub mod installation;
mod search;
pub mod storage;
pub mod utils;

use anyhow::Result;

#[cfg(windows)]
use crate::command::cli_alias;
use crate::{
    cli::{Cli, SubCommand},
    command::{cli_info, cli_install, cli_remove, cli_update},
    context::Context,
};

pub async fn dispatch(cli: Cli, ctx: Context) -> Result<()> {
    match cli.command {
        SubCommand::Install {
            opts,
            quiet,
            dry_run,
        } => {
            cli_install(&ctx.with_dry_run(dry_run).with_quiet(quiet), opts).await
        }
        SubCommand::Remove { packages, soft } => cli_remove(&ctx, packages, soft).await,
        SubCommand::Update {
            packages,
            local,
            interactive,
        } => cli_update(&ctx, packages, local, interactive).await,
        #[cfg(windows)]
        SubCommand::Alias { new_name, old_name } => cli_alias(&ctx, old_name, new_name).await,
        SubCommand::Info { packages } => cli_info(&ctx, packages).await,
    }
}
