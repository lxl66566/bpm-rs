//! A fork of the [trauma](https://github.com/rgreinho/trauma) crate, providing
//! a simple way to download files asynchronously via HTTP(S), tailored for bpm.
//!
//! Major changes compared to upstream (this fork does not follow upstream):
//! - the `reqwest-middleware` stack (`reqwest-tracing`, `opentelemetry`, `reqwest-retry`) is
//!   removed; retries are implemented in-house with exponential backoff;
//! - the global (overall) progress bar is removed: each asset keeps its own progress bar.

pub mod download;
pub mod downloader;

use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE};
use thiserror::Error;

/// Errors that can happen when using this crate.
#[derive(Error, Debug)]
pub enum Error {
    /// Error from the underlying URL parser or the expected URL format.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

/// Extension trait for `reqwest::header::HeaderMap`
///
/// Provide convenient methods for accessing common headers values.
pub(crate) trait ResponseExt {
    /// Get the content length from the headers.
    ///
    /// Returns None if the "content-length" header is missing or if its value
    /// is not a u64.
    fn content_length_header(&self) -> Option<u64>;

    /// Check whether the server supports range requests.
    ///
    /// Returns false if the "accept-ranges" header is missing or if its value
    /// is "none".
    fn accept_ranges(&self) -> bool;

    /// Get the content disposition header from the headers.
    ///
    /// Returns None if the "content-disposition" header is missing or if its
    /// value cannot be parsed as a string.
    fn content_disposition(&self) -> Option<String>;

    /// Get the total size from the "content-range" header (`bytes N-M/T`).
    ///
    /// Returns None if the header is missing, malformed, or the total is
    /// unknown (`*`).
    fn content_range_total(&self) -> Option<u64>;
}

impl ResponseExt for reqwest::Response {
    fn content_length_header(&self) -> Option<u64> {
        self.headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    }

    fn accept_ranges(&self) -> bool {
        match self.headers().get(ACCEPT_RANGES) {
            None => false,
            Some(x) if x == "none" => false,
            Some(_) => true,
        }
    }

    fn content_disposition(&self) -> Option<String> {
        self.headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    }

    fn content_range_total(&self) -> Option<u64> {
        parse_content_range_total(self.headers().get(CONTENT_RANGE)?.to_str().ok()?)
    }
}

/// Parse the total size out of a "content-range" header value.
///
/// For a 206 response, `Content-Length` is the remaining length, not the total
/// size; the total lives in the `Content-Range` value (`bytes N-M/T`), so only
/// the segment after the last `/` is needed.
pub(crate) fn parse_content_range_total(value: &str) -> Option<u64> {
    // Strictly `bytes N-M/T` (or `bytes */T`); anything else is malformed.
    let (unit, total) = value.rsplit_once('/')?;
    if !unit.trim().starts_with("bytes") {
        return None;
    }
    let total = total.trim();
    if total == "*" {
        return None;
    }
    total.parse().ok()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_content_range_total() {
        assert_eq!(parse_content_range_total("bytes 100-999/1234"), Some(1234));
        // Unknown total.
        assert_eq!(parse_content_range_total("bytes 100-999/*"), None);
        // Malformed values must not parse into a bogus total.
        assert_eq!(parse_content_range_total("garbage"), None);
        assert_eq!(parse_content_range_total("1234"), None);
        assert_eq!(parse_content_range_total("bytes 0-0/0"), Some(0));
    }
}
