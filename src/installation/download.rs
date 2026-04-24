use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use trauma::{
    download::Download,
    downloader::{DownloaderBuilder, ProgressBarOpts, StyleOptions},
};
use url::Url;

use crate::storage::Repo;

pub async fn download(repos: Vec<&Repo>, to: impl Into<PathBuf>) -> Result<Vec<PathBuf>> {
    let to = to.into();
    let mut filenames = vec![];

    // 稍微优化了提取逻辑，避免多重嵌套的 Option
    let assets = repos
        .into_iter()
        .filter_map(|repo| {
            let url_str = repo.asset.as_ref()?; // 如果没有 asset 直接跳过
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
        .collect::<Vec<_>>();

    let ret = filenames.into_iter().map(|x| to.join(x)).collect();

    // 2. 定义进度条样式，{msg} 代表 trauma 传递的文件名
    // 你可以根据喜好调整外观，例如加入颜色 (.cyan 等)
    let style = StyleOptions::new(
        ProgressBarOpts::new(
            Some(ProgressBarOpts::TEMPLATE_BAR_WITH_POSITION.into()),
            Some(ProgressBarOpts::CHARS_LINE.into()),
             true,
            false,
        ),
        ProgressBarOpts::new(
            Some(
                "{msg:<20.cyan} [{elapsed_precise}] [{wide_bar:.green/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})".into(),
            ),
            Some(ProgressBarOpts::CHARS_LINE.into()),
            true,
            false,
        ),
    );

    // 3. 将 style_options 应用到 Builder 中
    let downloader = DownloaderBuilder::new()
        .directory(to)
        .style_options(style)
        .build();

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
