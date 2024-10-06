use thiserror::Error;

#[allow(missing_docs, clippy::missing_docs_in_private_items)]
#[derive(Error, Debug)]
pub enum MyError {
    #[error("No available asset found in this repo. If you're sure there's a valid asset, use `--interactive`.")]
    NoAvailableAsset,
}
