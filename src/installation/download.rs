use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use trauma::{download::Download, downloader::DownloaderBuilder};
use url::Url;

use crate::storage::Repo;

pub async fn download(repos: Vec<&Repo>, to: impl Into<PathBuf>) -> Result<Vec<PathBuf>> {
    let to = to.into();
    let mut filenames = vec![];
    let assets = repos
        .into_iter()
        .filter_map(|repo| {
            repo.asset.as_ref().map(|url_str| {
                let url = Url::parse(url_str)
                    .map_err(|_| anyhow!("Invalid asset URL: {url_str}"))
                    .ok()?;
                let url_path = url.path_segments()?.next_back()?;
                let ext = Path::new(url_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default();
                let mut filename = repo.name.clone();
                filename.push('.');
                filename.push_str(ext);
                filenames.push(filename.clone());
                Some(Download::new(&url, &filename))
            })
        })
        .flatten()
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
        let mut repo = Repo::new("download_test");
        repo.asset =
            Some("https://github.com/seanmonstar/reqwest/archive/refs/tags/v0.1.0.zip".to_string());
        let result = download(vec![&repo], tempdir.path()).await;
        if let Ok(paths) = result {
            assert!(paths.iter().any(|p| p.exists()));
        }
    }
}
