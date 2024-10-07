use std::path::{Path, PathBuf};

use anyhow::Result;
use log::warn;
use tap::Tap;
use trauma::{download::Download, downloader::DownloaderBuilder};
use url::Url;

use crate::{storage::Repo, utils::UrlOps};

/// download select repos to a directory.
///
/// # Returns
///
/// a Vec of [`PathBuf`]s of downloaded files.
pub async fn download(repos: Vec<&Repo>, to: impl Into<PathBuf>) -> Result<Vec<PathBuf>> {
    let to = to.into();
    let mut filenames = vec![];
    let assets = repos
        .into_iter()
        .filter_map(|repo| {
            repo.asset
                .as_ref()
                .tap(|f| {
                    if f.is_none() {
                        warn!("Asset is not found: {}", repo.name);
                    }
                })
                .map(|url_str| {
                    let url = Url::parse(url_str).expect("parsing invalid URL.");
                    let mut filename = repo.name.clone();
                    let ext = Path::new(
                        url.path_segments()
                            .expect("url should has path")
                            .last()
                            .expect("url should has filename"),
                    )
                    .extension()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default();
                    filename.push('.');
                    filename.push_str(ext);

                    filenames.push(filename.clone());
                    Download::new(&url, &filename)
                })
        })
        .collect::<Vec<_>>();

    let ret = filenames.into_iter().map(|x| to.join(x)).collect();
    let downloader = DownloaderBuilder::new().directory(to).build();
    downloader.download(&assets).await;
    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_download() {
        let tempdir = tempfile::tempdir().unwrap();
        let mut repo = Repo::new("reqwest_test");
        repo.asset =
            Some("https://github.com/seanmonstar/reqwest/archive/refs/tags/v0.1.0.zip".to_string());
        let _ = download(vec![&repo], &tempdir.path()).await;
        assert!(tempdir.path().join("reqwest_test.zip").exists());
    }
}
