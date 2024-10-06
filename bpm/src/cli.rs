use std::path::PathBuf;

use clap::{ArgAction, Parser};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None, after_help = r#"Examples:
urldecoder test/t.md    # decode test/t.md
urldecoder *.md -e my   # decode all markdown files in current folder except which in `my` folder
urldecoder *            # decode all files in current folder
"#)]
pub struct Cli {
    /// Files to convert. It uses glob("**/{file}") to glob given pattern, like python's `rglob`
    file: PathBuf,
    /// Show result only without overwrite
    #[arg(short, long)]
    dry_run: bool,
    /// Show full error message
    #[arg(short, long)]
    verbose: bool,
    /// Exclude file or folder
    #[arg(short, long, action = ArgAction::Append)]
    exclude: Vec<PathBuf>,
    /// Do not decode `%20` to space
    #[arg(long)]
    escape_space: bool,
}
