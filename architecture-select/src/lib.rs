#![doc = include_str!("../README.md")]

/// Marker keywords for a target OS, named per `std::env::consts::OS`.
/// Split out so marker sets can be table-tested on any host.
fn platform_markers_for(os: &'static str) -> Vec<&'static str> {
    match os {
        "linux" => vec!["linux", "unix"],
        "windows" => vec!["windows", "win32"],
        "macos" => vec!["osx", "macos", "darwin"],
        // Match the exact BSD flavor; sharing markers would let a FreeBSD
        // host pick up NetBSD/OpenBSD-only assets.
        "freebsd" => vec!["freebsd"],
        "netbsd" => vec!["netbsd"],
        "openbsd" => vec!["openbsd"],
        other => vec![other],
    }
}

/// Marker keywords for a target arch, named per `std::env::consts::ARCH`.
fn architecture_markers_for(arch: &'static str) -> Vec<&'static str> {
    match arch {
        "x86_64" => vec!["x64", "x86_64", "amd64"],
        // "arm64" is the dominant spelling in release asset names
        // (darwin-arm64, linux-arm64, win-arm64); "aarch64" is the rustc
        // name and the minority spelling.
        "aarch64" => vec!["aarch64", "arm64", "armv8"],
        // "386" is the Go-style spelling for 32-bit x86.
        "x86" => vec!["x86", "i386", "386", "i686"],
        other => vec![other],
    }
}

fn platform_markers_with_pos_for(os: &'static str) -> Vec<(&'static str, MatchPos)> {
    let mut origin: Vec<_> = platform_markers_for(os)
        .into_iter()
        .map(|item| (item, MatchPos::default()))
        .collect();
    if os == "windows" {
        origin.push((".exe", MatchPos::End));
        origin.push((".msi", MatchPos::End));
    }
    origin
}

fn architecture_markers_with_pos_for(arch: &'static str) -> Vec<(&'static str, MatchPos)> {
    architecture_markers_for(arch)
        .into_iter()
        .map(|item| (item, MatchPos::default()))
        .collect()
}

fn platform_markers_with_pos() -> Vec<(&'static str, MatchPos)> {
    platform_markers_with_pos_for(std::env::consts::OS)
}

fn architecture_markers_with_pos() -> Vec<(&'static str, MatchPos)> {
    architecture_markers_with_pos_for(std::env::consts::ARCH)
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

/// Placement of matched items in [`sort_list`] output. Makes "best-first" an
/// explicit, compile-time contract instead of a boolean reversal flag.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum MatchOrder {
    /// Matched items first: the result is best-first.
    #[default]
    MatchedFirst,
    /// Matched items last.
    MatchedLast,
}

fn match_inner(
    item: &str,
    prompts_with_match_pos: &[(&str, MatchPos)],
    combination: Combination,
    case_sensitive: bool,
) -> bool {
    // Lowercase the target once instead of allocating per prompt.
    if case_sensitive {
        prompts_with_match_pos
            .iter()
            .any_or_all(combination, |(prompt, match_position)| {
                match_position.exec(item, prompt)
            })
    } else {
        let item_lower = item.to_lowercase();
        prompts_with_match_pos
            .iter()
            .any_or_all(combination, |(prompt, match_position)| {
                match_position.exec(&item_lower, &prompt.to_lowercase())
            })
    }
}

/// Filter items by matching against prompts.
///
/// # Arguments
/// * `combination` - All: every prompt must match. Any: at least one prompt matches.
/// * `case_sensitive` - true: exact case. false: case-insensitive (default in [`select`]).
pub fn select_list<S: AsRef<str>>(
    select_list: impl IntoIterator<Item = S>,
    prompts_with_match_pos: &[(&str, MatchPos)],
    combination: Combination,
    case_sensitive: bool,
) -> Vec<S> {
    select_list
        .into_iter()
        .filter(|item| {
            match_inner(
                item.as_ref(),
                prompts_with_match_pos,
                combination,
                case_sensitive,
            )
        })
        .collect()
}

/// Sort items so that matched items come first (or last, per `order`).
///
/// The sort is stable and never reverses the whole list: the relative order
/// inside the matched/unmatched groups is preserved.
///
/// # Arguments
/// * `combination` - All: every prompt must match. Any: at least one matches.
/// * `case_sensitive` - true: exact case. false: case-insensitive.
/// * `order` - where matched items are placed.
pub fn sort_list<S: AsRef<str>>(
    mut sort_list: Vec<S>,
    prompts_with_match_pos: &[(&str, MatchPos)],
    combination: Combination,
    case_sensitive: bool,
    order: MatchOrder,
) -> Vec<S> {
    sort_list.sort_by_key(|item| {
        let matched = match_inner(
            item.as_ref(),
            prompts_with_match_pos,
            combination,
            case_sensitive,
        );
        match order {
            MatchOrder::MatchedFirst => !matched,
            MatchOrder::MatchedLast => matched,
        }
    });
    sort_list
}

/// Select the best match item from assets based on current platform and
/// architecture.
///
/// Returns a **best-first** list: non-matching assets are dropped (hard
/// filter), and known-useless suffixes (`.pdb`, `.dll`, checksums, ...) are
/// sorted to the end. Non-interactive callers must take the first element.
pub fn select<S: AsRef<str>>(assets: Vec<S>) -> Vec<S> {
    // Select platform (case-insensitive)
    let assets = select_list(
        assets,
        &platform_markers_with_pos(),
        Combination::Any,
        false,
    );

    // Select architecture (case-insensitive)
    let assets = select_list(
        assets,
        &architecture_markers_with_pos(),
        Combination::Any,
        false,
    );

    // Sort unwanted suffixes to the end
    sort_list(
        assets,
        [".pdb", ".dll", ".txt", ".checksum", ".sha", ".sha256"]
            .map(|item| (item, MatchPos::End))
            .as_ref(),
        Combination::Any,
        false,
        MatchOrder::MatchedLast,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn test_markers_with_pos() {
        assert_eq!(platform_markers_with_pos(), vec![
            ("windows", MatchPos::All),
            ("win32", MatchPos::All),
            (".exe", MatchPos::End),
            (".msi", MatchPos::End)
        ]);
    }

    #[test]
    fn test_architecture_markers_table() {
        assert_eq!(architecture_markers_for("x86_64"), [
            "x64", "x86_64", "amd64"
        ]);
        assert_eq!(architecture_markers_for("aarch64"), [
            "aarch64", "arm64", "armv8"
        ]);
        assert_eq!(architecture_markers_for("x86"), [
            "x86", "i386", "386", "i686"
        ]);
        assert_eq!(architecture_markers_for("riscv64"), ["riscv64"]);
    }

    #[test]
    fn test_platform_markers_table() {
        assert_eq!(platform_markers_for("linux"), ["linux", "unix"]);
        assert_eq!(platform_markers_for("windows"), ["windows", "win32"]);
        assert_eq!(platform_markers_for("macos"), ["osx", "macos", "darwin"]);
        // Each BSD flavor matches only its own name.
        assert_eq!(platform_markers_for("freebsd"), ["freebsd"]);
        assert_eq!(platform_markers_for("netbsd"), ["netbsd"]);
        assert_eq!(platform_markers_for("openbsd"), ["openbsd"]);
    }

    /// Table-driven check of the platform+arch filters on arbitrary targets:
    /// (os, arch, asset, expected to survive both filters).
    #[test]
    fn test_select_filters_by_explicit_platform_arch() {
        let table = [
            ("macos", "aarch64", "typstyle-darwin-arm64", true),
            ("macos", "aarch64", "typstyle-macos-arm64", true),
            ("linux", "aarch64", "typstyle-linux-arm64", true),
            ("windows", "aarch64", "typstyle-win32-arm64.exe", true),
            ("windows", "aarch64", "typstyle-win32-arm64.pdb", true),
            ("linux", "x86", "tool-linux-386.tar.gz", true),
            ("linux", "x86", "tool-linux-i686.tar.gz", true),
            ("linux", "x86_64", "tool-linux-amd64.tar.gz", true),
            ("windows", "x86_64", "tool-win32-x64.exe", true),
            ("freebsd", "x86_64", "tool-freebsd-amd64.txz", true),
            ("netbsd", "x86_64", "tool-netbsd-amd64.txz", true),
            // wrong arch
            ("macos", "x86_64", "typstyle-darwin-arm64", false),
            ("linux", "x86_64", "typstyle-linux-arm64", false),
            ("windows", "aarch64", "typstyle-win32-x64.exe", false),
            // wrong os
            ("linux", "x86_64", "tool-win32-x64.zip", false),
            // BSD flavors must not cross-match
            ("freebsd", "x86_64", "tool-netbsd-amd64.txz", false),
            ("freebsd", "x86_64", "tool-openbsd-amd64.txz", false),
        ];
        for (os, arch, asset, expected) in table {
            let platform = platform_markers_with_pos_for(os);
            let arch_markers = architecture_markers_with_pos_for(arch);
            let survived = select_list(
                select_list([asset], &platform, Combination::Any, false),
                &arch_markers,
                Combination::Any,
                false,
            )
            .len()
                == 1;
            assert_eq!(survived, expected, "os={os} arch={arch} asset={asset}");
        }
    }

    #[test]
    fn test_any_or_all() {
        let mut iter = vec![1, 2, 3].into_iter();
        assert!(iter.any_or_all(Combination::Any, |item| item == 1));
        assert!(!iter.any_or_all(Combination::All, |item| item == 1));
    }

    #[test]
    fn test_match_inner_case_insensitive() {
        assert!(match_inner(
            "asdFoo123",
            &[("foo", MatchPos::All)],
            Combination::Any,
            false,
        ));
        assert!(match_inner(
            "ASDFOO123",
            &[("foo", MatchPos::All)],
            Combination::Any,
            false,
        ));
    }

    #[test]
    fn test_match_inner_case_sensitive() {
        assert!(match_inner(
            "asdfoo123",
            &[("foo", MatchPos::All)],
            Combination::Any,
            true,
        ));
        assert!(!match_inner(
            "ASDFOO123",
            &[("foo", MatchPos::All)],
            Combination::Any,
            true,
        ));
    }

    #[test]
    fn test_match_inner_positions() {
        assert!(!match_inner(
            "asdfoo123",
            &[("foo", MatchPos::Begin)],
            Combination::Any,
            false,
        ));
        assert!(match_inner(
            "asdfoo123",
            &[("asd", MatchPos::Begin)],
            Combination::Any,
            false,
        ));
        assert!(match_inner(
            "asdfoo123",
            &[("123", MatchPos::End)],
            Combination::Any,
            false,
        ));
    }

    #[test]
    fn test_select_platform() {
        let assets = ["65windowsasd", "asdlasflinuxsad12", "flaksjddarwina165"]
            .map(ToOwned::to_owned)
            .to_vec();

        let select = select_list(
            assets,
            &platform_markers_with_pos(),
            Combination::Any,
            false,
        );
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
        let sorted = sort_list(
            assets.clone(),
            &[("a", MatchPos::All)],
            Combination::Any,
            false,
            MatchOrder::MatchedFirst,
        );
        assert_eq!(sorted, ["bar", "baz", "foo"]);
        // MatchedLast must not reverse the whole list: within-group order is
        // preserved (this full-reversal is what caused the worst-first pick).
        let sorted = sort_list(
            assets,
            &[("a", MatchPos::All)],
            Combination::Any,
            false,
            MatchOrder::MatchedLast,
        );
        assert_eq!(sorted, ["foo", "bar", "baz"]);
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
        // select() is best-first: the first element must be the pick.
        if cfg!(windows) {
            if cfg!(target_arch = "aarch64") {
                assert_eq!(selected_assets[0], "typstyle-win32-arm64.exe");
            } else {
                assert_eq!(selected_assets[0], "typstyle-win32-x64.exe");
            }
        } else if cfg!(target_os = "linux") {
            if cfg!(target_arch = "aarch64") {
                assert_eq!(selected_assets[0], "typstyle-linux-arm64");
            } else {
                assert_eq!(selected_assets[0], "typstyle-linux-x64");
            }
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                assert_eq!(selected_assets[0], "typstyle-darwin-arm64");
            } else {
                assert_eq!(selected_assets[0], "typstyle-darwin-x64");
            }
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
