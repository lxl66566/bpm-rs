pub mod constants;
pub mod err;

use std::path::{Path, PathBuf};
use url::Url;

/// join given [`Path`]s as posix path.
pub fn path_join(paths: impl IntoIterator<Item = impl AsRef<Path>>) -> PathBuf {
    paths.into_iter().fold(PathBuf::new(), |acc, p| acc.join(p))
}

/// Join all strings to a [`Url`] object.
pub trait UrlJoinAll<'a> {
    fn join_all<I: IntoIterator<Item = String>>(&self, paths: I) -> Result<Url, url::ParseError>;
    fn join_all_str<I: IntoIterator<Item = &'a str>>(
        &self,
        paths: I,
    ) -> Result<Url, url::ParseError>;
}

impl<'a> UrlJoinAll<'a> for Url {
    fn join_all<I: IntoIterator<Item = String>>(&self, paths: I) -> Result<Url, url::ParseError> {
        let mut url = self.clone();
        for mut path in paths {
            if !path.ends_with('/') {
                path.push('/');
            }
            url = url.join(path.as_str())?;
        }
        Ok(url)
    }
    fn join_all_str<I: IntoIterator<Item = &'a str>>(
        &self,
        paths: I,
    ) -> Result<Url, url::ParseError> {
        self.join_all(paths.into_iter().map(|s| s.to_string()))
    }
}

/// Format a repo as a row in info list.
pub fn fmt_repo_list<T, U, V>(name: T, url: U, version: V) -> String
where
    T: AsRef<str>,
    U: AsRef<str>,
    V: AsRef<str>,
{
    format!(
        "{:20}{:50}{:20}",
        name.as_ref(),
        url.as_ref(),
        version.as_ref()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_join_all() {
        let base_url = Url::parse("https://codegeex.cn").unwrap();
        let paths = ["foo", "bar", "baz/asdf"];
        let url = base_url
            .join_all(paths.iter().map(|s| s.to_string()))
            .unwrap();
        assert_eq!(
            url.as_str().trim_matches('/'),
            "https://codegeex.cn/foo/bar/baz/asdf"
        );
    }
}
