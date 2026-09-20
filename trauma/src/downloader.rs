//! Represents the download controller.

use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

use bon::Builder;
use futures::stream::{self, StreamExt};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use reqwest::{
    Client, RequestBuilder, Response, StatusCode,
    header::{HeaderMap, RANGE},
};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

use crate::{
    ResponseExt,
    download::{Download, Status, Summary},
};

const DEFAULT_RETRIES: u32 = 3;
const DEFAULT_CONCURRENT_DOWNLOADS: usize = 32;
/// Base delay for the exponential backoff between retries, in milliseconds.
const RETRY_BASE_DELAY_MS: u64 = 500;

/// Represents the download controller.
///
/// A downloader can be created via its builder:
///
/// ```rust
/// # fn main()  {
/// use bpm_trauma::downloader::Downloader;
///
/// let d = Downloader::builder().build();
/// # }
/// ```
#[derive(Debug, Clone, Builder)]
pub struct Downloader {
    /// Directory where to store the downloaded files.
    #[builder(default = std::env::current_dir().unwrap_or_default(), into)]
    directory: PathBuf,
    /// Number of retries per request.
    #[builder(default = DEFAULT_RETRIES)]
    retries: u32,
    /// Number of maximum concurrent downloads.
    #[builder(default = DEFAULT_CONCURRENT_DOWNLOADS)]
    concurrent_downloads: usize,
    /// Downloader style options.
    #[builder(default)]
    style_options: StyleOptions,
    /// Resume the download if necessary and possible.
    #[builder(default = true)]
    resumable: bool,
    /// Custom HTTP headers.
    headers: Option<HeaderMap>,
    /// HTTP client shared by all `download()` calls, so the connection pool
    /// and TLS sessions are reused instead of being rebuilt per call.
    /// Initialized lazily on first use because it must incorporate `headers`,
    /// which a builder field default cannot reference.
    /// `download_with_proxy` cannot use it: a Client is immutable once built,
    /// so a per-call proxy needs a dedicated client.
    #[builder(skip)]
    client: OnceLock<Client>,
}

impl Default for Downloader {
    fn default() -> Self {
        Downloader::builder().build()
    }
}

impl Downloader {
    const ALREADY_DOWNLOADED: &str = "the file was already fully downloaded";

    /// Starts the downloads.
    pub async fn download(&self, downloads: &[Download]) -> Vec<Summary> {
        let client = self.shared_client();
        self.download_inner(downloads, &client).await
    }

    /// Starts the downloads with proxy.
    pub async fn download_with_proxy(
        &self,
        downloads: &[Download],
        proxy: reqwest::Proxy,
    ) -> Vec<Summary> {
        // A Client cannot be reconfigured after build, so a per-call proxy
        // rules out the shared client; build a dedicated one.
        let client = self.build_client(Some(proxy));
        self.download_inner(downloads, &client).await
    }

    /// Get the client shared by all `download()` calls, creating it once.
    fn shared_client(&self) -> Client {
        self.client.get_or_init(|| self.build_client(None)).clone()
    }

    /// Build an HTTP client honoring the configured headers and proxy.
    fn build_client(&self, proxy: Option<reqwest::Proxy>) -> Client {
        let mut client_builder = Client::builder();
        if let Some(proxy) = proxy {
            client_builder = client_builder.proxy(proxy);
        }
        if let Some(headers) = &self.headers {
            client_builder = client_builder.default_headers(headers.clone());
        }
        client_builder.build().expect("the client to build")
    }

    /// Starts the downloads.
    async fn download_inner(&self, downloads: &[Download], client: &Client) -> Vec<Summary> {
        // Prepare the progress bars container.
        // There is no global (overall) progress bar: each asset gets its own.
        let multi = if self.style_options.is_hidden() {
            MultiProgress::with_draw_target(ProgressDrawTarget::hidden())
        } else {
            MultiProgress::new()
        };
        let multi = Arc::new(multi);

        // Download the files asynchronously.
        stream::iter(downloads)
            .map(|d| self.fetch(client, d, multi.clone()))
            .buffer_unordered(self.concurrent_downloads)
            .collect::<Vec<_>>()
            .await
    }

    /// Send a request with retries and exponential backoff.
    ///
    /// Retries network errors and transient HTTP statuses (429 and 5xx). The
    /// last outcome is returned as-is when the retries are exhausted.
    async fn send_with_retry(
        &self,
        build_request: impl Fn() -> RequestBuilder,
    ) -> Result<Response, reqwest::Error> {
        let mut attempt = 0;
        loop {
            let outcome = build_request().send().await;
            let should_retry = match &outcome {
                Ok(res) => Self::is_transient_status(res.status()),
                Err(_) => true,
            };
            if !should_retry || attempt >= self.retries {
                return outcome;
            }
            let delay = Duration::from_millis(RETRY_BASE_DELAY_MS << attempt.min(6));
            log::debug!(
                "Request failed (attempt {}/{}), retrying in {:?}...",
                attempt + 1,
                self.retries,
                delay
            );
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }

    /// Check whether an HTTP status is transient and worth retrying.
    fn is_transient_status(status: StatusCode) -> bool {
        status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
    }

    /// Fetches the files and write them to disk.
    async fn fetch(
        &self,
        client: &Client,
        download: &Download,
        multi: Arc<MultiProgress>,
    ) -> Summary {
        // Create a download summary.
        let summary = Summary::builder().download(download.clone());

        // Retrieve download metadata.
        let response = match self.send_with_retry(|| client.head(download.url())).await {
            Ok(r) => r,
            Err(e) => {
                return summary.status(Status::Fail(e.to_string())).build();
            },
        };

        // Try to build the output path.
        let Some(filename) = download.infer_filename(&response) else {
            return summary
                .status(Status::Fail(
                    "Cannot extract the filename. Verify the URL and/or provide an override."
                        .to_string(),
                ))
                .build();
        };
        let output = self.directory.join(&filename);
        log::debug!("Filename: {filename}");

        // Check if there is a file on disk already.
        let size_on_disk: u64 = match tokio::fs::metadata(&output).await {
            Ok(m) => {
                log::debug!("A file with the same name already exists at the destination.");
                // If so, check file length to know where to restart the download from.
                m.len()
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => {
                return summary.status(Status::Fail(e.to_string())).build();
            },
        };

        // Determine whether the download is resumable or not.
        let can_resume = self.resumable && Download::is_resumable(&response);

        // Update the summary accordingly.
        let summary = summary.resumable(can_resume);

        // Whether the GET request below will ask for the remaining bytes.
        let range_requested = can_resume;

        // Check whether or not we need to download the file.
        let content_length_value = response.content_length_header().unwrap_or_default();
        if size_on_disk > 0 && size_on_disk == content_length_value {
            return summary
                .status(Status::Skipped(Self::ALREADY_DOWNLOADED.into()))
                .build();
        }

        // Build the GET request.
        log::debug!("Fetching {}", download.url_as_str());
        let res = match self
            .send_with_retry(|| {
                let mut req = client.get(download.url_as_str());
                // If resumable is turned on, request the remaining bytes.
                if range_requested {
                    req = req.header(RANGE, format!("bytes={size_on_disk}-"));
                }
                // Add extra headers if needed.
                if let Some(ref h) = self.headers {
                    req = req.headers(h.to_owned());
                }
                req
            })
            .await
        {
            Ok(res) => res,
            Err(e) => {
                return summary.status(Status::Fail(e.to_string())).build();
            },
        };

        // Check the status for errors.
        match res.error_for_status_ref() {
            Ok(_res) => (),
            Err(e) => return summary.status(Status::Fail(e.to_string())).build(),
        };

        // Decide how the response body relates to the partial file on disk.
        let decision = ResumeDecision::new(range_requested, &res, size_on_disk);
        if decision.append {
            log::debug!("Resuming download at byte {size_on_disk}.");
        } else if range_requested {
            log::debug!(
                "Server ignored the Range request ({}); restarting from scratch.",
                res.status()
            );
        }

        // Update the summary with the collected details.
        let summary = summary.statuscode(res.status());

        // If there is nothing else to download for this file, we can return.
        if size_on_disk > 0 && size_on_disk == decision.total {
            return summary
                .size(decision.total)
                .status(Status::Skipped(Self::ALREADY_DOWNLOADED.into()))
                .build();
        }

        // Create the progress bar for this asset.
        // The bar covers the whole file: when resuming, it starts at the
        // on-disk offset; when restarting from scratch, it starts at zero.
        let child_opts = self.style_options.child.clone();

        // Add the progress bar to the multi progress container.
        let pb = multi.add(
            child_opts
                .to_progress_bar(decision.total)
                .with_position(decision.start)
                .with_message(filename),
        );

        // Prepare the destination directory.
        let output_dir = output.parent().unwrap_or(&output);
        log::debug!("Creating destination directory {:?}", output_dir);
        if let Err(e) = tokio::fs::create_dir_all(output_dir).await {
            return summary
                .size(decision.total)
                .status(Status::Fail(e.to_string()))
                .build();
        }

        // Prepare the destination file.
        log::debug!("Creating destination file {output:?}");
        let mut file = match OpenOptions::new()
            .create(true)
            .write(true)
            .append(decision.append)
            .truncate(!decision.append)
            .open(output)
            .await
        {
            Ok(file) => file,
            Err(e) => {
                return summary
                    .size(decision.total)
                    .status(Status::Fail(e.to_string()))
                    .build();
            },
        };

        // Prepare the final size.
        // We will add the amount of bytes downloaded to the amount of bytes
        // that are already on disk (or to zero when restarting from scratch).
        let mut final_size = decision.start;

        // Download the file chunk by chunk.
        log::debug!("Retrieving chunks...");
        let mut stream = res.bytes_stream();
        while let Some(item) = stream.next().await {
            // Retrieve chunk.
            let mut chunk = match item {
                Ok(chunk) => chunk,
                Err(e) => {
                    return summary
                        .size(final_size)
                        .status(Status::Fail(e.to_string()))
                        .build();
                },
            };
            let chunk_size = chunk.len() as u64;
            final_size += chunk_size;
            pb.inc(chunk_size);

            // Write the chunk to disk.
            match file.write_all_buf(&mut chunk).await {
                Ok(_res) => (),
                Err(e) => {
                    return summary
                        .size(final_size)
                        .status(Status::Fail(e.to_string()))
                        .build();
                },
            };
        }

        // Finish the progress bar once complete, and optionally remove it.
        if self.style_options.child.clear {
            pb.finish_and_clear();
        } else {
            pb.finish();
        }

        // Return a successful summary with the actual download size.
        summary.size(final_size).status(Status::Success).build()
    }
}

/// How to treat the GET response body relative to the partial file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResumeDecision {
    /// Append the response body to the partial file instead of truncating it.
    append: bool,
    /// Offset the progress bar and the final size start from.
    start: u64,
    /// Total file size, used for the skip check and the progress bar total.
    total: u64,
}

impl ResumeDecision {
    /// Evaluate the GET response against the partial file on disk.
    fn new(range_requested: bool, res: &Response, size_on_disk: u64) -> Self {
        Self::decide(
            range_requested,
            res.status(),
            res.content_range_total(),
            res.content_length_header(),
            size_on_disk,
        )
    }

    /// Pure decision core, kept free of `Response` for table-driven tests.
    ///
    /// Only a `206 Partial Content` response to our Range request carries the
    /// remaining bytes; a server that ignores Range replies `200` with the
    /// full body, which must replace the partial file instead of being
    /// appended to it, or the result would be silently corrupt.
    fn decide(
        range_requested: bool,
        status: StatusCode,
        content_range_total: Option<u64>,
        content_length: Option<u64>,
        size_on_disk: u64,
    ) -> Self {
        let append = range_requested && status == StatusCode::PARTIAL_CONTENT;
        let remaining = content_length.unwrap_or_default();
        let total = if append {
            // For a 206, Content-Length is the remaining length, not the
            // total; the total lives in Content-Range (`bytes N-M/T`), with
            // offset + remaining as the fallback when the header is missing.
            content_range_total.unwrap_or(size_on_disk + remaining)
        } else {
            remaining
        };
        Self {
            append,
            start: if append {
                size_on_disk
            } else {
                0
            },
            total,
        }
    }
}

/// Define the [`Downloader`] options.
///
/// By default, the progress bars are cleared from the screen upon completion.
#[derive(Debug, Clone, Builder)]
pub struct StyleOptions {
    /// Style options for the per-asset progress bar(s).
    child: ProgressBarOpts,
}

impl Default for StyleOptions {
    fn default() -> Self {
        Self::builder()
            .child(ProgressBarOpts::with_pip_style())
            .build()
    }
}

impl StyleOptions {
    /// Convenience function to create a style hidding the progress bars.
    pub fn hidden() -> Self {
        Self::builder().child(ProgressBarOpts::hidden()).build()
    }

    /// Set the options for the child progress bar.
    pub fn set_child(&mut self, child: ProgressBarOpts) {
        self.child = child;
    }

    /// Check whether the progress bars are hidden.
    pub fn is_hidden(&self) -> bool {
        self.child.hidden
    }
}

/// Define the options for a progress bar.
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into))]
pub struct ProgressBarOpts {
    /// Progress bar template string.
    template: Option<String>,
    /// Progression characters set.
    ///
    /// There must be at least 3 characters for the following states:
    /// "filled", "current", and "to do".
    progress_chars: Option<String>,
    /// Hide the progress bar.
    #[builder(default, with = || true)]
    hidden: bool,
    /// Clear the progress bar once completed.
    #[builder(default, with = || true)]
    clear: bool,
}

impl Default for ProgressBarOpts {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl ProgressBarOpts {
    /// Use increasing quarter blocks as progress characters: `"█▛▌▖  "`.
    pub const CHARS_BLOCKY: &'static str = "█▛▌▖  ";
    /// Use fade-in blocks as progress characters: `"█▓▒░  "`.
    pub const CHARS_FADE_IN: &'static str = "█▓▒░  ";
    /// Use fine blocks as progress characters: `"█▉▊▋▌▍▎▏  "`.
    pub const CHARS_FINE: &'static str = "█▉▊▋▌▍▎▏  ";
    /// Use a line as progress characters: `"━╾─"`.
    pub const CHARS_LINE: &'static str = "━╾╴─";
    /// Use rough blocks as progress characters: `"█  "`.
    pub const CHARS_ROUGH: &'static str = "█  ";
    /// Use increasing height blocks as progress characters: `"█▇▆▅▄▃▂▁  "`.
    pub const CHARS_VERTICAL: &'static str = "█▇▆▅▄▃▂▁  ";
    /// Template which looks like the Python package installer pip.
    ///
    /// `━━━━━╾──────────────    3.78 MiB/13.43 MiB    191.39 KiB/s eta 52s  archive.zip`
    pub const TEMPLATE_PIP: &'static str = "{bar:20.green/black} \
                                            {bytes:>11.green}/{total_bytes:<11.green} \
                                            {bytes_per_sec:>13.red} eta {eta:4.blue} {msg}";

    /// Create a [`ProgressStyle`] based on the provided options.
    pub fn to_progress_style(self) -> ProgressStyle {
        let mut style = ProgressStyle::default_bar();
        if let Some(template) = self.template {
            style = style.template(&template).unwrap();
        }
        if let Some(progress_chars) = self.progress_chars {
            style = style.progress_chars(&progress_chars);
        }
        style
    }

    /// Create a [`ProgressBar`] based on the provided options.
    pub fn to_progress_bar(self, len: u64) -> ProgressBar {
        // Return a hidden Progress bar if we hid it.
        if self.hidden {
            return ProgressBar::hidden();
        }

        // Otherwise returns a ProgressBar with the style.
        let style = self.to_progress_style();
        ProgressBar::new(len).with_style(style)
    }

    /// Create a new [`ProgressBarOpts`] which looks like Python pip.
    pub fn with_pip_style() -> Self {
        Self::builder()
            .template(ProgressBarOpts::TEMPLATE_PIP)
            .progress_chars(ProgressBarOpts::CHARS_LINE)
            .clear()
            .build()
    }

    /// Create a new [`ProgressBarOpts`] which hides the progress bars.
    pub fn hidden() -> Self {
        Self::builder().hidden().build()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let d = Downloader::builder().build();
        assert_eq!(d.retries, DEFAULT_RETRIES);
        assert_eq!(d.concurrent_downloads, DEFAULT_CONCURRENT_DOWNLOADS);
        assert!(d.resumable);
    }

    #[test]
    fn test_builder_resumable_toggle() {
        let d = Downloader::builder().resumable(false).build();
        assert!(!d.resumable);
    }

    #[test]
    fn test_style_options_defaults() {
        let s = StyleOptions::default();
        assert!(!s.is_hidden());
        assert!(s.child.clear);
    }

    #[test]
    fn test_style_options_hidden() {
        let s = StyleOptions::hidden();
        assert!(s.is_hidden());
    }

    #[test]
    fn test_is_transient_status() {
        assert!(Downloader::is_transient_status(
            StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(Downloader::is_transient_status(
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(!Downloader::is_transient_status(StatusCode::NOT_FOUND));
        assert!(!Downloader::is_transient_status(StatusCode::OK));
    }

    #[test]
    fn test_resume_decision() {
        // Regression (P0-5): a 200 response to a Range request carries the
        // full body; appending it to the partial file would corrupt it.
        assert_eq!(
            ResumeDecision::decide(true, StatusCode::OK, None, Some(100), 40),
            ResumeDecision {
                append: false,
                start: 0,
                total: 100,
            }
        );
        // 206: append at the on-disk offset; the total comes from
        // Content-Range since Content-Length is only the remaining length.
        assert_eq!(
            ResumeDecision::decide(true, StatusCode::PARTIAL_CONTENT, Some(100), Some(60), 40),
            ResumeDecision {
                append: true,
                start: 40,
                total: 100,
            }
        );
        // 206 without Content-Range: total = offset + remaining. This also
        // pins the old false-skip bug: disk == remaining (40 == 40) must not
        // look like a completed download of 40 bytes.
        assert_eq!(
            ResumeDecision::decide(true, StatusCode::PARTIAL_CONTENT, None, Some(40), 40),
            ResumeDecision {
                append: true,
                start: 40,
                total: 80,
            }
        );
        // No Range requested: plain full download, restart at zero even when
        // a stale partial file exists on disk.
        assert_eq!(
            ResumeDecision::decide(false, StatusCode::OK, None, Some(100), 40),
            ResumeDecision {
                append: false,
                start: 0,
                total: 100,
            }
        );
    }
}
