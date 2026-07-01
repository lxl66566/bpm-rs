use std::path::PathBuf;

use anyhow::{Result, anyhow};
use log::info;
use trauma::{
    download::Download,
    downloader::{Downloader, ProgressBarOpts, StyleOptions},
};
use url::Url;

use crate::{storage::Repo, utils::FileNameExt};

pub async fn download(repos: Vec<&Repo>, to: impl Into<PathBuf>) -> Result<Vec<(String, PathBuf)>> {
    let to = to.into();
    let mut ret = vec![];

    let assets = repos
        .into_iter()
        .filter_map(|repo| {
            let url_str = repo.asset.as_ref()?;
            info!("Downloading `{}` from {url_str}", repo.name);

            let url = Url::parse(url_str)
                .map_err(|_| anyhow!("Invalid asset URL: {url_str}"))
                .ok()?;

            let url_path = url.path_segments()?.next_back()?;
            let ext = url_path.preserve_extension();

            let mut filename = repo.name.clone();
            filename.push_str(ext);

            ret.push((repo.name.clone(), to.join(filename.clone())));

            Some(
                Download::builder()
                    .url(url.as_str())
                    .ok()?
                    .filename_override(filename)
                    .build(),
            )
        })
        .collect::<Vec<_>>();

    // 2. 定义进度条样式
    let style = StyleOptions::builder()
        .main(
            ProgressBarOpts::builder()
                .template(ProgressBarOpts::TEMPLATE_BAR_WITH_POSITION)
                .progress_chars(ProgressBarOpts::CHARS_LINE)
                .build(),
        )
        .child(
            ProgressBarOpts::builder()
                .template("{msg:<20.cyan} [{elapsed_precise}] [{wide_bar:.green/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                .progress_chars(ProgressBarOpts::CHARS_LINE)
                .build(),
        )
        .build();

    let downloader = Downloader::builder()
        .directory(&to)
        .style_options(style)
        .build();

    let summaries = downloader.download(&assets).await;

    for summary in &summaries {
        if let trauma::download::Status::Fail(msg) = summary.status() {
            log::warn!("下载失败: {msg}");
        }
    }

    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires network access to GitHub"]
    async fn test_download() {
        let tempdir = tempfile::tempdir().unwrap();
        let mut repo = Repo::new("download_test");
        repo.asset =
            Some("https://github.com/seanmonstar/reqwest/archive/refs/tags/v0.1.0.zip".to_string());
        let result = download(vec![&repo], tempdir.path()).await;
        if let Ok(paths) = result {
            assert!(paths.iter().any(|(_, p)| p.exists()));
        }
    }
}
