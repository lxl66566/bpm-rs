# bpm-rs

English | [简体中文](./README-zh_CN.md)

Previously the [Python version of bin-package-manager](https://github.com/lxl66566/bpm), this is the RIIR version and actively developed version, compatible with the original configuration and data storage, allowing seamless switching from the Python version.

bpm (bin package manager) is a package manager based on GitHub Releases, allowing users to install and manage binary files from any GitHub Release. Supports General Linux and Windows.

> [!CAUTION]
> Risk Warning: The Linux version of bpm carries a potential risk of damaging your computer. By using bpm to install software, you acknowledge this risk and trust the third-party packagers of the GitHub Release.

> [!TIP]
> bpm guarantees idempotence, meaning that running `bpm uninstall` immediately after `bpm install` will not make any changes to the system.

### Comparison

- Unlike scoop/pacman, which need packagers to maintain packaging scripts, bpm is fully automatic and handles ~95% of GitHub Releases out of the box.
- Unlike cargo-binstall (Rust/crates.io only), bpm is language-agnostic with built-in repo search and automatic asset selection. Rust apps in particular install with a near-100% success rate.

## Installation

### Bootstrap (recommended)

Install the latest version with a single command:

```sh
# Unix (Linux / macOS), requires root privileges
curl -fsSL https://raw.githubusercontent.com/lxl66566/bpm-rs/main/bootstrap.sh | sudo sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/lxl66566/bpm-rs/main/bootstrap.ps1 | iex
```

The script downloads the appropriate release archive, extracts the `bpm` binary, and installs it via `bpm install --local`.

### Pre-built binaries

Download the binary for your platform from [Releases](https://github.com/lxl66566/bpm-rs/releases) and place it in your PATH.

### Build from source

```sh
cargo install bpm --git https://github.com/lxl66566/bpm-rs
```

## Usage

```sh
bpm i <package>         # install (from GitHub search, user/repo, or URL)
bpm i <package> --local <archive|dir>   # install from a local file
bpm r <package>         # remove
bpm u [package]         # update one / all
bpm list                # list installed packages (--json, --outdated)
bpm doctor              # verify installed files
bpm -h                  # show more help
```

## Configuration

Optional config file at `~/.config/bpm/config.toml` (override with `--config`):

```toml
# install_position = "D:/bpm"      # root for app/bin (db moves with it)
# db_path = "D:/bpm/db.json"       # explicit db location
# prefix = "/usr/local"            # unix install prefix (--prefix wins)
# github_token = "ghp_xxx"         # higher API rate limit; GITHUB_TOKEN/GH_TOKEN env wins
```

## How it works

- **Asset selection**: bpm matches platform/arch/libc keywords to pick the best asset automatically; use `--interactive` to choose manually.
- **Binary matching**: after extraction, bpm looks for the binary by (1) using the only file if the archive contains exactly one, (2) otherwise recursively matching `bin_name` (defaults to the package name, `--bin-name`/`-b` to override, `.exe` appended on Windows), (3) on Windows, falling back to all `.exe` files. `--one-bin` enforces single-binary installs (Linux only).

### Linux

bpm detects the file structure inside the asset and installs to the corresponding system locations:

1. Install binaries to `/usr/bin`
2. Merge `lib/`, `include/`, `share/`, `man/`, `bin/` directories into the system `usr/` (if they exist)
3. Install completions
4. Install services (for systemd-based systems)

Existing files are renamed to `<name>.old` instead of being overwritten, and restored on uninstall.

Because it relies on the [FHS](https://en.wikipedia.org/wiki/Filesystem_Hierarchy_Standard), bpm is not suitable for distributions like NixOS that do not follow FHS.

### Windows

bpm extracts everything to `%userprofile%/bpm/app/<name>` and creates scoop-style shims in `%userprofile%/bpm/bin` (added to PATH automatically) to launch the executables, avoiding cmd/sh compatibility issues. Single `.exe` or `.msi` assets are also supported.
