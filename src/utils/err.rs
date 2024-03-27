use std::process;

use die_exit::PrintExit;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("No available asset found in this repo. If you're sure there's a valid asset, use `--interactive`.")]
    NoAvailableAsset,
}

impl PrintExit for MyError {
    fn print_exit(&self) -> ! {
        eprintln!("Error: {}", self);
        process::exit(1);
    }
}
