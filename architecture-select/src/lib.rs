fn platform_markers() -> Vec<&'static str> {
    if cfg!(target_os = "linux") {
        vec!["linux", "unix"]
    } else if cfg!(target_os = "windows") {
        vec!["windows", "win32"]
    } else if cfg!(target_os = "macos") {
        vec!["osx", "macos", "darwin"]
    } else if cfg!(target_os = "freebsd") {
        vec!["freebsd", "netbsd", "openbsd"]
    } else {
        vec![std::env::consts::OS]
    }
}

fn architecture_markers() -> Vec<&'static str> {
    if cfg!(target_arch = "x86_64") {
        vec!["x64", "x86_64", "amd64"]
    } else if cfg!(target_arch = "aarch64") {
        vec!["aarch64", "armv8"]
    } else if cfg!(target_arch = "x86") {
        vec!["x86", "i386", "i686"]
    } else {
        vec![std::env::consts::ARCH]
    }
}

fn platform_markers_with_pos() -> Vec<(&'static str, MatchPos)> {
    let mut origin: Vec<_> = platform_markers()
        .into_iter()
        .map(|item| (item, MatchPos::default()))
        .collect();
    if cfg!(target_os = "windows") {
        origin.push((".exe", MatchPos::End));
        origin.push((".msi", MatchPos::End));
    }
    origin
}

fn architecture_markers_with_pos() -> Vec<(&'static str, MatchPos)> {
    architecture_markers()
        .into_iter()
        .map(|item| (item, MatchPos::default()))
        .collect()
}

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum Combination {
    All,
    #[default]
    Any,
}

trait CombinationTrait<T>: Sized {
    fn any_or_all(&mut self, comb: Combination, func: impl FnMut(T) -> bool) -> bool;
}

impl<T, I> CombinationTrait<T> for I
where
    I: Iterator<Item = T>,
{
    fn any_or_all(&mut self, comb: Combination, func: impl FnMut(T) -> bool) -> bool {
        match comb {
            Combination::All => self.all(func),
            Combination::Any => self.any(func),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum MatchPos {
    #[default]
    All,
    Begin,
    End,
}

impl MatchPos {
    fn exec(&self, target: &str, pattern: &str) -> bool {
        self.func()(pattern, target)
    }

    fn func(&self) -> impl Fn(&str, &str) -> bool {
        match self {
            MatchPos::All => |pattern: &str, target: &str| target.contains(pattern),
            MatchPos::Begin => |pattern: &str, target: &str| target.starts_with(pattern),
            MatchPos::End => |pattern: &str, target: &str| target.ends_with(pattern),
        }
    }
}

fn match_inner(
    item: &str,
    prompts_with_match_pos: &[(&str, MatchPos)],
    combination: Option<Combination>,
    case_sensitive: Option<bool>,
) -> bool {
    prompts_with_match_pos.iter().any_or_all(
        combination.unwrap_or_default(),
        |(prompt, match_position)| {
            if case_sensitive.unwrap_or(false) {
                match_position.exec(item.to_lowercase().as_str(), prompt.to_lowercase().as_str())
            } else {
                match_position.exec(item, prompt)
            }
        },
    )
}

pub fn select_list(
    select_list: impl IntoIterator<Item = String>,
    prompts_with_match_pos: &[(&str, MatchPos)],
    combination: Option<Combination>,
    case_sensitive: Option<bool>,
) -> Vec<String> {
    select_list
        .into_iter()
        .filter(|item| match_inner(item, prompts_with_match_pos, combination, case_sensitive))
        .collect()
}

pub fn sort_list(
    mut sort_list: Vec<String>,
    prompts_with_match_pos: &[(&str, MatchPos)],
    combination: Option<Combination>,
    case_sensitive: Option<bool>,
    reverse: Option<bool>,
) -> Vec<String> {
    sort_list.sort_by_key(|item| {
        !match_inner(item, prompts_with_match_pos, combination, case_sensitive)
    });
    if reverse.unwrap_or(false) {
        sort_list.reverse();
    }
    sort_list
}

pub fn select(assets: Vec<String>) -> Vec<String> {
    // Select platform
    let assets = select_list(assets, platform_markers_with_pos().as_ref(), None, None);

    // Select architecture
    let assets = select_list(assets, architecture_markers_with_pos().as_ref(), None, None);

    // I don't want these suffix
    let assets = sort_list(
        assets,
        [".pdb", ".dll", ".txt", ".checksum", ".sha", ".sha256"]
            .map(|item| (item, MatchPos::End))
            .as_ref(),
        None,
        None,
        Some(true),
    );

    assets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn test_markers_with_pos() {
        assert_eq!(
            platform_markers_with_pos(),
            vec![
                ("windows", MatchPos::All),
                ("win32", MatchPos::All),
                (".exe", MatchPos::End),
                (".msi", MatchPos::End)
            ]
        );
    }

    #[test]
    fn test_any_or_all() {
        let mut iter = vec![1, 2, 3].into_iter();
        assert!(iter.any_or_all(Combination::Any, |item| item == 1));
        assert!(!iter.any_or_all(Combination::All, |item| item == 1));
    }

    #[test]
    fn test_match_inner() {
        assert!(match_inner(
            "asdfoo123",
            &[("foo", MatchPos::All)],
            None,
            None
        ));

        assert!(!match_inner(
            "asdfoo123",
            &[("foo", MatchPos::Begin)],
            None,
            None
        ));
        assert!(match_inner(
            "asdfoo123",
            &[("asd", MatchPos::Begin)],
            None,
            None
        ));

        assert!(match_inner(
            "asdfoo123",
            &[("123", MatchPos::End)],
            None,
            None
        ));
    }

    #[test]
    fn test_select_platform() {
        let assets = ["65windowsasd", "asdlasflinuxsad12", "flaksjddarwina165"]
            .map(ToOwned::to_owned)
            .to_vec();

        let select = select_list(assets, platform_markers_with_pos().as_ref(), None, None);
        assert!(!select.is_empty());
        if cfg!(windows) {
            assert_eq!(select[0], "65windowsasd");
        } else if cfg!(target_os = "linux") {
            assert_eq!(select[0], "asdlasflinuxsad12");
        } else if cfg!(target_os = "macos") {
            assert_eq!(select[0], "flaksjddarwina165");
        }
    }

    #[test]
    fn test_sort_list() {
        let assets = ["foo", "bar", "baz"].map(ToOwned::to_owned).to_vec();
        let sorted = sort_list(assets, &[("a", MatchPos::All)], None, None, None);
        assert_eq!(sorted, ["bar", "baz", "foo"]);
    }

    #[test]
    fn test_select_typstyle() {
        let assets = [
            "typstyle-alpine-x64",
            "typstyle-alpine-x64.debug",
            "typstyle-darwin-arm64",
            "typstyle-darwin-arm64.dwarf",
            "typstyle-darwin-x64",
            "typstyle-darwin-x64.dwarf",
            "typstyle-linux-arm64",
            "typstyle-linux-arm64.debug",
            "typstyle-linux-armhf",
            "typstyle-linux-armhf.debug",
            "typstyle-linux-x64",
            "typstyle-linux-x64.debug",
            "typstyle-win32-arm64.exe",
            "typstyle-win32-arm64.pdb",
            "typstyle-win32-x64.exe",
            "typstyle-win32-x64.pdb",
        ]
        .map(ToOwned::to_owned)
        .to_vec();

        let selected_assets = select(assets.clone());
        assert!(!selected_assets.is_empty());
        if cfg!(windows) {
            assert_eq!(selected_assets[0], "typstyle-win32-x64.exe");
        }
    }

    #[test]
    fn manual_test() {
        let assets = [
            "asd-macos-x86_64-123.zip",
            "asd-windows-x86_64-123.zip",
            "asd-linux-x86_64-123.zip",
            "asd-macos-aarch64-123.zip",
            "asd-windows-aarch64-123.zip",
            "asd-linux-aarch64-123.zip",
        ]
        .map(ToOwned::to_owned)
        .to_vec();

        let selected_assets = select(assets);
        println!("Selected asset: {:?}", selected_assets);
    }
}
