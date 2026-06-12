# bpm-rs

English | [简体中文](./README-zh_CN.md)

Previously the [Python version of bin-package-manager](https://github.com/lxl66566/bpm), this is the RIIR version and actively developed version, compatible with the original configuration and data storage, allowing seamless switching from the Python version.

bpm (bin package manager) is a package manager based on GitHub Releases, allowing users to install and manage binary files from any GitHub Release. Supports General Linux and Windows.

> [!CAUTION]
> Risk Warning: The Linux version of bpm carries a potential risk of damaging your computer. By using bpm to install software, you acknowledge this risk and trust the third-party packagers of the GitHub Release.

> [!TIP]
> bpm guarantees idempotence, meaning that running `bpm uninstall` immediately after `bpm install` will not make any changes to the system.

### compare to

- Differences from scoop/pacman: Tools like scoop/pacman require packagers to manually maintain packaging scripts; bpm is a fully automated tool that can handle 95% of GitHub Releases without additional packaging maintenance.
- Differences from cargo-binstall: cargo-binstall can only install Rust applications, and if the Rust package is not published on crates.io, installation can be cumbersome. bpm is language-agnostic, integrating repository search and automatic asset selection for greater convenience. Since Rust packaging and release processes are fairly standardized, bpm has nearly a 100% success rate for installing Rust applications.

## Installation

### Pre-built binaries

Download the binary for your platform from [Releases](https://github.com/lxl66566/bpm-rs/releases) and place it in your PATH.

### Build from source

```sh
cargo install bpm --git https://github.com/lxl66566/bpm-rs
```

## Usage

```sh
bpm i <package>       # install
bpm list              # list installed packages
bpm -h                # show more help
```

## How it works

- Asset selection: bpm has a relatively complex asset matching mechanism that generally selects the best asset as the installation target. If no suitable asset can be found, you can use `--interactive` to force asset selection.
- Binary matching logic: After decompression, bpm searches for the target binary file in the following order: (1) If there is only one file in the archive, use that file directly; (2) Otherwise, recursively scan all files to match `bin_name` (defaults to the package name, can be overridden by `--bin-name` / `-b`, and automatically appends the `.exe` suffix on Windows); (3) On Windows, if no match is found, fall back to using all `.exe` files. (4) `--one-bin` is used to enforce single-file mode (Linux only, installs only the first matched binary).

### Linux

bpm automatically detects the file structure within the asset and installs to the corresponding system locations:

1. Install binaries to `/usr/bin`
2. Merge `lib/`, `include/`, `share/`, `man/`, `bin/` directories into the system `usr/` (if they exist)
3. Install completions
4. Install services (for systemd-based systems)

bpm automatically appends a `.old` suffix to existing files to avoid overwriting; upon uninstallation, the `.old` files are restored.

Because it relies on the [FHS](https://en.wikipedia.org/wiki/Filesystem_Hierarchy_Standard), bpm is not suitable for distributions like NixOS that do not follow FHS.

### Windows

bpm downloads and extracts all contents to `%userprofile%/bpm/app/<name>`, and creates startup shims for executable files in `%userprofile%/bpm/bin` (this location is automatically added to PATH). When matching assets, it also supports downloading and installing a single `.exe` file or `.msi` file.

On Windows, bpm uses a shims mechanism similar to scoop, launching installed software via an exe proxy to avoid compatibility issues with cmd/sh etc. (The original Python version does not support this feature.)
