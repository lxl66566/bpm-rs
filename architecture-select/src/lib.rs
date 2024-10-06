use std::collections::HashMap;
use std::sync::LazyLock as Lazy;

static PLATFORM_MARKERS: Lazy<HashMap<&'static str, Vec<&'static str>>> = Lazy::new(|| {
    HashMap::from([
        ("win", vec!["windows", "win"]),
        ("linux", vec!["linux"]),
        ("darwin", vec!["osx", "darwin"]),
        ("freebsd", vec!["freebsd", "netbsd", "openbsd"]),
    ])
});

static NON_AMD64_MARKERS: [&str; 15] = [
    "i386", "i686", "arm", "arm64", "386", "ppc64", "armv7", "armv7l", "mips64", "ppc64",
    "mips64le", "ppc64le", "aarch64", "armhf", "armv7hl",
];
