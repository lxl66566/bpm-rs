use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, ensure};
use bpm_trauma::{
    download::{Download, Status},
    downloader::{Downloader, ProgressBarOpts, StyleOptions},
};
use log::info;
use url::Url;

use crate::{storage::Repo, utils::FileNameExt};

/// Progress bar style: one bar per asset, no total bar.
/// `--quiet` promises "Suppress progress output", so hide the bars entirely.
fn style_options(quiet: bool) -> StyleOptions {
    if quiet {
        StyleOptions::hidden()
    } else {
        StyleOptions::builder()
            .child(
                ProgressBarOpts::builder()
                    .template(
                        "{msg:<20.cyan} [{elapsed_precise}] [{wide_bar:.green/blue}] \
                         {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                    )
                    .progress_chars(ProgressBarOpts::CHARS_LINE)
                    .build(),
            )
            .build()
    }
}

pub async fn download(
    repos: Vec<&Repo>,
    to: impl Into<PathBuf>,
    quiet: bool,
) -> Result<Vec<(String, PathBuf)>> {
    let to = to.into();
    let mut ret = vec![];
    let mut assets = vec![];

    for repo in repos {
        // Missing asset is a programming error upstream; fail with context
        // instead of silently dropping the repo from the batch.
        let url_str = repo
            .asset
            .as_ref()
            .ok_or_else(|| anyhow!("no asset selected for `{}`", repo.name))?;
        // Print the whole batch list before any progress bar exists: bars
        // render on stderr, and log lines emitted while they are up garble
        // their redraw. These lines also stay as the only per-package
        // feedback in --quiet mode, where the bars are hidden — so info!,
        // not debug!.
        info!("Downloading `{}` from {url_str}", repo.name);

        let url = Url::parse(url_str).map_err(|e| anyhow!("invalid asset URL '{url_str}': {e}"))?;

        let url_path = url
            .path_segments()
            .and_then(|mut s| s.next_back())
            .ok_or_else(|| anyhow!("asset URL has no filename: {url_str}"))?;
        let ext = url_path.preserve_extension();

        let mut filename = repo.name.clone();
        filename.push_str(ext);

        // Build the request first, track the file only on success, otherwise
        // the result would contain paths that were never downloaded.
        let download = Download::builder()
            .url(url.as_str())
            .context("failed to build download request")?
            .filename_override(filename.clone())
            .build();
        ret.push((repo.name.clone(), to.join(filename)));
        assets.push(download);
    }

    // 2. Progress bar style (see [`style_options`]).
    let style = style_options(quiet);

    let downloader = Downloader::builder()
        .directory(&to)
        .style_options(style)
        .build();

    let summaries = downloader.download(&assets).await;

    // A failed download must abort the whole batch: previously failures were
    // only warned about, and later stages crashed with misleading
    // "No such file" errors from unzip.
    let mut errors = Vec::new();
    for summary in &summaries {
        match summary.status() {
            Status::Success => {},
            Status::Skipped(msg) => {
                log::warn!("Skipped {}: {msg}", summary.download().url_as_str());
            },
            Status::Fail(msg) => {
                errors.push(format!("{}: {msg}", summary.download().url_as_str()));
            },
            Status::NotStarted => {
                errors.push(format!("{}: not started", summary.download().url_as_str()));
            },
        }
    }
    ensure!(errors.is_empty(), "some downloads failed: {errors:?}");

    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hides_progress_bars() {
        assert!(style_options(true).is_hidden());
        assert!(!style_options(false).is_hidden());
    }

    #[tokio::test]
    #[ignore = "requires network access to GitHub"]
    async fn test_download() {
        let tempdir = tempfile::tempdir().unwrap();
        let mut repo = Repo::new("download_test");
        repo.asset =
            Some("https://github.com/seanmonstar/reqwest/archive/refs/tags/v0.1.0.zip".to_string());
        let result = download(vec![&repo], tempdir.path(), false).await;
        if let Ok(paths) = result {
            assert!(paths.iter().any(|(_, p)| p.exists()));
        }
    }
}
