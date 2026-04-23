use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code, clippy::enum_variant_names)]
pub enum BpmError {
    #[error("Repository '{0}' not found")]
    RepoNotFound(String),

    #[error("Repository '{0}' is already installed")]
    AlreadyInstalled(String),

    #[error("No available asset found for '{0}'. Try --interactive.")]
    InvalidAsset(String),

    #[error("Release has no assets for {0}/{1}")]
    AssetNotFound(String, String),

    #[error("GitHub API error: {0}")]
    ApiError(String),

    #[error("Package '{0}' not installed")]
    PackageNotInstalled(String),

    #[error("Binary file not found in {0}")]
    BinaryNotFound(PathBuf),

    #[error("Unsafe removal path: {0}")]
    UnsafeRemoval(PathBuf),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type BpmResult<T> = Result<T, BpmError>;
