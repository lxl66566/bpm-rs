use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, ValueHint};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: SubCommand,
    #[arg(short, long, value_hint(ValueHint::FilePath))]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SubCommand {
    #[clap(visible_alias("i"))]
    Install {
        #[clap(required = true)]
        packages: Vec<String>,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        bin_name: Option<String>,
        #[arg(
            short,
            long,
            value_hint(ValueHint::FilePath),
            value_name = "LOCAL_PATH"
        )]
        local: Option<PathBuf>,
        #[arg(short, long, conflicts_with = "interactive")]
        quiet: bool,
        #[arg(long)]
        one_bin: bool,
        #[arg(long)]
        prefer_gnu: bool,
        #[arg(short, long)]
        dry_run: bool,
        #[arg(short, long)]
        interactive: bool,
        #[arg(long)]
        filter: Vec<String>,
        #[arg(long)]
        #[clap(default_value = "best-match")]
        sort: SortParam,
    },

    #[clap(visible_alias("r"))]
    Remove {
        #[clap(required = true)]
        packages: Vec<String>,
        #[arg(short, long)]
        soft: bool,
    },

    #[clap(visible_alias("u"))]
    Update {
        packages: Vec<String>,
        #[arg(
            short,
            long,
            value_hint(ValueHint::FilePath),
            value_name = "LOCAL_PATH"
        )]
        local: Option<PathBuf>,
    },

    #[cfg(windows)]
    #[clap(visible_alias("a"))]
    Alias { new_name: String, old_name: String },

    #[clap(visible_alias("list"), visible_alias("l"))]
    Info { packages: Vec<String> },
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
