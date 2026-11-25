//! Download cache: archives of failed installs are kept so a retry skips
//! re-downloading them; a successful install drops the package's entries.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use log::{info, warn};
use url::Url;

use crate::{installation::download::asset_filename, utils::path::PathExt};

/// Cache layout: `{root}/{package}/{url-hash}-{asset-filename}`.
///
/// Entries are keyed by the asset URL: a new release gets a fresh entry, and
/// a re-released asset never reuses a stale file. Restores copy the entry
/// under the canonical download filename, so the cache key never leaks into
/// the extraction output.
#[derive(Debug)]
pub struct DownloadCache {
    root: PathBuf,
}

impl DownloadCache {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Cache entry path for the asset. Pure computation, so [`Self::get`]
    /// and [`Self::store`] share it as the single key.
    #[must_use]
    pub fn entry(&self, name: &str, url: &str) -> PathBuf {
        // Parse through the same helper as download naming, so a URL query
        // string cannot leak into a Windows-illegal filename.
        let asset = Url::parse(url)
            .ok()
            .and_then(|u| asset_filename(&u).ok())
            .unwrap_or_else(|| String::from("asset"));
        self.root
            .join(name)
            .join(format!("{:016x}-{asset}", url_hash(url)))
    }

    /// Existing cache entry for the asset, if any.
    #[must_use]
    pub fn get(&self, name: &str, url: &str) -> Option<PathBuf> {
        let entry = self.entry(name, url);
        entry.is_file().then_some(entry)
    }

    /// Copy a downloaded archive into the cache. An existing entry is kept
    /// as-is: the same asset is never cached twice.
    pub fn store(&self, name: &str, url: &str, src: impl AsRef<Path>) -> Result<()> {
        let entry = self.entry(name, url);
        if entry.is_file() {
            return Ok(());
        }
        if let Some(parent) = entry.parent() {
            parent.create_dir_if_not_exist()?;
        }
        fs::copy(src.as_ref(), &entry)?;
        info!("Cached download of `{name}`.");
        Ok(())
    }

    /// Drop every cache entry of the package: a successful install must
    /// leave nothing cached. Best-effort — a cache cleanup failure must
    /// not fail the install.
    pub fn clear(&self, name: &str) {
        if let Err(e) = self.root.join(name).remove_all_allow_missing() {
            warn!("Failed to clear download cache of `{name}`: {e}");
        }
    }

    /// Drop the entry of an asset that failed to extract: a poison entry
    /// must not break every future retry.
    pub fn invalidate(&self, name: &str, url: &str) {
        if let Err(e) = self.entry(name, url).remove_all_allow_missing() {
            warn!("Failed to drop cache entry of `{name}`: {e}");
        }
    }

    /// Remove the cache dirs of packages that are no longer installed.
    /// Entries are otherwise dropped only by a successful install, so
    /// abandoned retries would accumulate forever. Returns the removed
    /// package names.
    pub fn clean_orphans(&self, is_installed: impl Fn(&str) -> bool) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| {
                let e = e.ok()?;
                let name = e.file_name().into_string().ok()?;
                if is_installed(&name) {
                    return None;
                }
                self.root.join(&name).remove_all_allow_missing().ok()?;
                Some(name)
            })
            .collect()
    }
}

/// FNV-1a 64-bit over the URL. Custom instead of std's `DefaultHasher`: its
/// algorithm is unspecified and may change between Rust versions, which
/// would silently orphan existing entries.
fn url_hash(url: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://github.com/o/r/releases/download/v1.0/tool-linux.tar.gz";

    #[test]
    fn url_hash_is_stable() {
        // Pin the exact values: an accidental algorithm change would orphan
        // every existing cache entry.
        assert_eq!(url_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(url_hash("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn entry_filename_excludes_query_string() {
        let cache = DownloadCache::new("/cache");
        let entry = cache.entry(
            "tool",
            "https://github.com/o/r/releases/download/v1.0/tool-linux.tar.gz?token=x",
        );
        let filename = entry.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!filename.contains('?'));
        assert!(filename.ends_with("tool-linux.tar.gz"));
    }

    #[test]
    fn store_get_invalidate_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DownloadCache::new(tmp.path().join("cache"));

        assert!(cache.get("tool", URL).is_none());

        let src = tmp.path().join("src.tar.gz");
        fs::write(&src, "archive").unwrap();
        cache.store("tool", URL, &src).unwrap();
        let entry = cache.get("tool", URL).expect("stored entry must be found");
        assert_eq!(fs::read(&entry).unwrap(), b"archive");

        // The same asset is never cached twice: a second store must not
        // overwrite the existing entry.
        let second = tmp.path().join("second.tar.gz");
        fs::write(&second, "other").unwrap();
        cache.store("tool", URL, &second).unwrap();
        assert_eq!(fs::read(&entry).unwrap(), b"archive");

        // A different release URL gets its own entry.
        let url_v2 = "https://github.com/o/r/releases/download/v2.0/tool-linux.tar.gz";
        cache.store("tool", url_v2, &second).unwrap();
        assert_ne!(cache.get("tool", url_v2).unwrap(), entry);

        // Invalidate drops only the matching entry.
        cache.invalidate("tool", URL);
        assert!(cache.get("tool", URL).is_none());
        assert!(cache.get("tool", url_v2).is_some());

        // Clear drops the package's whole entry dir.
        cache.clear("tool");
        assert!(cache.get("tool", url_v2).is_none());
    }

    #[test]
    fn clean_orphans_drops_only_uninstalled() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DownloadCache::new(tmp.path().join("cache"));

        let src = tmp.path().join("src.tar.gz");
        fs::write(&src, "archive").unwrap();
        cache.store("gone", URL, &src).unwrap();
        cache.store("kept", URL, &src).unwrap();

        let removed = cache.clean_orphans(|name| name == "kept");
        assert_eq!(removed, ["gone"]);
        assert!(cache.get("gone", URL).is_none());
        assert!(cache.get("kept", URL).is_some());
    }
}
