use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, ValueHint};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: SubCommand,
    /// Path to the config file
    #[arg(short, long, value_hint(ValueHint::FilePath))]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SubCommand {
    /// Install a package
    #[clap(visible_alias("i"))]
    Install {
        /// Package names or GitHub repos to install (e.g. `ripgrep`, `user/repo`, `https://github.com/user/repo`)
        #[clap(required = true)]
        packages: Vec<String>,
        /// Override the install name (defaults to the package name)
        #[arg(short, long)]
        name: Option<String>,
        /// Specify the binary file name to look for after extraction (defaults
        /// to package name)
        #[arg(short, long)]
        bin_name: Option<String>,
        /// Install from a local archive or directory instead of downloading
        #[arg(
            short,
            long,
            value_hint(ValueHint::FilePath),
            value_name = "LOCAL_PATH"
        )]
        local: Option<PathBuf>,
        /// Suppress progress output; conflicts with --interactive
        #[arg(short, long, conflicts_with = "interactive")]
        quiet: bool,
        /// Only install the matched binary, skip other files (Linux only)
        #[arg(long)]
        one_bin: bool,
        /// Prefer GNU builds over musl when selecting assets
        #[arg(long)]
        prefer_gnu: bool,
        /// Simulate the installation without making any changes
        #[arg(short, long)]
        dry_run: bool,
        /// Interactively choose which asset to download
        #[arg(short, long)]
        interactive: bool,
        /// Only consider assets containing all specified substrings
        #[arg(long)]
        filter: Vec<String>,
        /// Include pre-release versions when searching
        #[arg(long)]
        pre_release: bool,
        /// Sort method for search results
        #[arg(long)]
        #[clap(default_value = "best-match")]
        sort: SortParam,
    },

    /// Uninstall a package
    #[clap(visible_alias("r"))]
    Remove {
        /// Package names to remove
        #[clap(required = true)]
        packages: Vec<String>,
        /// Only remove symlinks/shim files, keep the extracted app directory
        #[arg(short, long)]
        soft: bool,
    },

    /// Update installed packages
    #[clap(visible_alias("u"))]
    Update {
        /// Package names to update (empty = update all)
        packages: Vec<String>,
        /// Install from a local archive or directory instead of downloading
        #[arg(
            short,
            long,
            value_hint(ValueHint::FilePath),
            value_name = "LOCAL_PATH"
        )]
        local: Option<PathBuf>,
    },

    /// Create an alias for an installed package (Windows only)
    #[cfg(windows)]
    #[clap(visible_alias("a"))]
    Alias {
        /// New alias name
        new_name: String,
        /// Existing package name to alias
        old_name: String,
    },

    /// List installed packages or show details
    #[clap(visible_alias("list"), visible_alias("l"))]
    Info {
        /// Package names to query (empty = list all)
        packages: Vec<String>,
    },
}

/// Sort method for search results
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
