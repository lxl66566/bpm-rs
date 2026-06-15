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
    /// Unix install prefix (default: /usr for root, ~/.local for non-root)
    #[arg(long, global = true, value_hint(ValueHint::DirPath))]
    pub prefix: Option<PathBuf>,
}

/// Options shared by install and update for selecting and downloading assets.
#[allow(clippy::struct_excessive_bools)]
#[derive(Parser, Debug, Clone)]
pub struct InstallOptions {
    /// Package names or GitHub repos to install (e.g. `ripgrep`, `user/repo`, `https://github.com/user/repo`)
    #[clap(required = true)]
    pub packages: Vec<String>,
    /// Override the install name (defaults to the package name)
    #[arg(short, long)]
    pub name: Option<String>,
    /// Specify the binary file name to look for after extraction (defaults
    /// to package name)
    #[arg(short, long)]
    pub bin_name: Option<String>,
    /// Install from a local archive or directory instead of downloading
    #[arg(
        short,
        long,
        value_hint(ValueHint::FilePath),
        value_name = "LOCAL_PATH"
    )]
    pub local: Option<PathBuf>,
    /// Only install the matched binary, skip other files (Linux only)
    #[arg(long)]
    pub one_bin: bool,
    /// Prefer musl builds over gnu when selecting assets (default: prefer
    /// gnu)
    #[arg(long)]
    pub prefer_musl: bool,
    /// Interactively choose which asset to download
    #[arg(short, long)]
    pub interactive: bool,
    /// Only consider assets containing all specified substrings
    #[arg(long)]
    pub filter: Vec<String>,
    /// Include pre-release versions when searching
    #[arg(long)]
    pub pre_release: bool,
    /// Sort method for search results
    #[arg(long)]
    #[clap(default_value = "best-match")]
    pub sort: SortParam,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SubCommand {
    /// Install a package
    #[clap(visible_alias("i"))]
    Install {
        #[command(flatten)]
        opts: InstallOptions,
        /// Suppress progress output; conflicts with --interactive
        #[arg(short, long, conflicts_with = "interactive")]
        quiet: bool,
        /// Simulate the installation without making any changes
        #[arg(short, long)]
        dry_run: bool,
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
        /// Interactively choose which asset to update to
        #[arg(short, long)]
        interactive: bool,
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
        /// Output in JSON format (for scripting)
        #[arg(long)]
        json: bool,
        /// Check for available updates (requires network)
        #[arg(long)]
        outdated: bool,
    },

    /// Verify installed package integrity
    #[clap(visible_alias("check"))]
    Doctor,
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
