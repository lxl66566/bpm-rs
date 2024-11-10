use std::{path::PathBuf, sync::RwLock};

use clap::{Parser, Subcommand, ValueEnum, ValueHint};

pub static DRY_RUN: RwLock<bool> = RwLock::new(false);

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None, after_help = r#"Examples:"#)]
#[clap(args_conflicts_with_subcommands = false)]
pub struct Cli {
    #[command(subcommand)]
    pub command: SubCommand,
    #[arg(short, long, value_hint(ValueHint::FilePath))]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SubCommand {
    /// Install packages.
    #[clap(alias("i"))]
    Install {
        /// The packages to install. You can specify package name or github url.
        #[clap(required = true)]
        packages: Vec<String>,

        /// Specify the binary executable filename, otherwise use package name by default.
        #[arg(short, long)]
        bin_name: Option<String>,

        /// Install from local archive.
        #[arg(
            short,
            long,
            value_hint(ValueHint::FilePath),
            value_name = "LOCAL_PATH"
        )]
        local: Option<PathBuf>,

        /// Do not ask, install the first repo in the search result, and show less messages.
        #[arg(short, long, conflicts_with = "interactive")]
        quiet: bool,

        /// Install given binary only. Use package name as binary name by default.
        #[arg(long)]
        one_bin: bool,

        /// Bpm prefers musl target by default. Use this flag to prefer gnu target.
        #[arg(long)]
        prefer_gnu: bool,

        /// Print the install result, but do not install actually.
        #[arg(short, long)]
        dry_run: bool,

        /// Select asset interactively.
        #[arg(short, long)]
        interactive: bool,

        /// Filter assets
        #[arg(long)]
        filter: Vec<String>,

        /// Sort param in github api.
        #[arg(long)]
        #[clap(default_value = "best-match")]
        sort: SortParam,
    },

    /// Remove packages.
    #[clap(alias("r"))]
    Remove {
        /// The packages to remove.
        #[clap(required = true)]
        packages: Vec<String>,

        /// Only remove item in database, do not delete softwares themselves.
        #[arg(short, long)]
        soft: bool,
    },

    /// Update packages.
    #[clap(alias("u"))]
    Update {
        /// The packages to update.
        packages: Vec<String>,

        /// Update from local archive.
        #[arg(
            short,
            long,
            value_hint(ValueHint::FilePath),
            value_name = "LOCAL_PATH"
        )]
        local: Option<PathBuf>,
    },

    /// Alias package executable (Windows only).
    #[cfg(windows)]
    #[clap(alias("a"))]
    Alias { new_name: String, old_name: String },

    /// Show packages info.
    #[clap(alias("list"), alias("l"))]
    Info {
        /// The packages to info. If empty, show all installed packages.
        packages: Vec<String>,
    },
}

#[derive(ValueEnum, strum_macros::AsRefStr, Clone, Copy, Debug, Eq, PartialEq, Default)]
#[strum(serialize_all = "kebab-case")]
pub enum SortParam {
    #[default]
    BestMatch,
    Stars,
    Forks,
    HelpWantedIssues,
    Updated,
}
