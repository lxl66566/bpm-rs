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
use cli::{Cli, SubCommand, DRY_RUN};

use search::SearchableSequence;
use storage::{Repo, RepoList};
use utils::{log::set_quiet_log, log_init};

#[tokio::main]
pub async fn main() -> Result<()> {
    log_init();
    let cli = Cli::parse();
    dbg!(&cli);

    Ok(())
}

impl SubCommand {
    async fn install(&self) -> Result<()> {
        if let Self::Install {
            packages,
            bin_name,
            local,
            quiet,
            one_bin,
            prefer_gnu,
            dry_run,
            interactive,
            filter,
            sort,
        } = self
        {
            if *quiet {
                set_quiet_log();
            }
            if *dry_run {
                *DRY_RUN.write().unwrap() = true;
            } else {
                #[cfg(unix)]
                {
                    assert!(
                        crate::utils::is_root(),
                        "You must run as root to install packages."
                    );
                }
            }

            let repo_list: RepoList = packages
                .iter()
                .map(|p| Repo::from(p.as_str()))
                .collect::<Vec<_>>()
                .into();
            let res = repo_list.pre_install(*quiet, *interactive, *sort).await;
            todo!();

            Ok(())
        } else {
            unreachable!()
        }
    }
}
