# bpm-rs

[English](./README.md) | 简体中文

前身为 [Python 版本的 bin-package-manager](https://github.com/lxl66566/bpm)，此为其 rust 重写版本与活跃开发版本，兼容原版配置与数据存储，可直接从 Python 版无缝切换。

bpm (bin package manager) 是一个基于 Github Release 的包管理器，允许用户安装并管理任意 Github Release 上的二进制文件。支持 General Linux 与 Windows。

> [!CAUTION]
> 风险提示：bpm Linux 版存在潜在的破坏计算机风险。使用 bpm 安装软件即代表您已接受此风险，并信任第三方 Github Release 的打包者。

> [!TIP]
> bpm 保证对偶性，即 `bpm install` 后立即接 `bpm uninstall` 不会对系统作出任何改变。

### compare to

- 与 scoop/pacman 的区别：scoop/pacman 等工具需要打包者手动维护打包脚本；bpm 属于全自动化工具，可以兼容 95% 的 Github Release 打包，不需要额外的打包维护。
- 与 cargo-binstall 的区别：cargo-binstall 只能安装 rust 编写的应用，且如果 rust 包没有在 crates.io 发布，安装会比较麻烦。bpm 不限语言，集成仓库搜索、自动判断 assets 为一体，使用更加方便。由于 rust 打包发布流程比较标准，bpm 安装 rust 应用的成功率几乎为 100%。

## 安装

### 自举安装（推荐）

一行命令安装最新版本：

```sh
# Unix (Linux / macOS)，需要 root 权限
curl -fsSL https://raw.githubusercontent.com/lxl66566/bpm-rs/main/bootstrap.sh | sudo sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/lxl66566/bpm-rs/main/bootstrap.ps1 | iex
```

脚本会下载对应平台的 release 压缩包，解压得到 `bpm` 二进制文件，然后通过 `bpm install --local` 进行安装。

### 预编译二进制

从 [Releases](https://github.com/lxl66566/bpm-rs/releases) 下载对应平台的二进制文件，并放到 path 中。

### 源码编译

```sh
cargo install bpm --git https://github.com/lxl66566/bpm-rs
```

## 使用

```sh
bpm i <package>       # 安装
bpm list              # 查看已安装的包
bpm -h                # 查看更多帮助
```

## 原理解释

- 选择 asset：bpm 有一套较为复杂的 assets 匹配机制，一般可以筛选出最佳的 asset 作为安装目标。如果无法筛选到合适 asset，您可以使用 `--interactive` 来强制选择 asset。
- binary 匹配逻辑：解压后，bpm 按以下顺序查找目标 bin 文件：(1) 如果压缩包内只有一个文件，直接使用该文件；(2) 否则递归扫描所有文件，匹配 `bin_name`（默认为包名，可通过 `--bin-name` / `-b` 覆盖，在 Windows 上自动补 `.exe` 后缀）；(3) 在 Windows 上若未匹配到，则回退使用所有 `.exe` 文件。 (4) `--one-bin` 用于强制单文件模式（Linux only，只安装第一个匹配到的 binary）。

### Linux

bpm 自动判断 asset 中的文件结构，并安装到系统中的相应位置：

1. 安装 binary 到 `/usr/bin`
2. 合并 `lib/`, `include/`, `share/`, `man/`, `bin/` 目录到系统 `usr/` 下（如果存在）
3. 安装 completions
4. 安装 services（基于 systemd 的系统）

bpm 自动为已存在的文件添加 `.old` 后缀以避免覆盖，卸载时 `.old` 文件将被恢复。

由于依赖 [FHS](https://en.wikipedia.org/wiki/Filesystem_Hierarchy_Standard)，bpm 不适用于 NixOS 等不遵循 FHS 的发行版。

### Windows

bpm 下载内容并全部解压到 `%userprofile%/bpm/app/<name>`，为其中的可执行文件创建启动 shims 到 `%userprofile%/bpm/bin`（该位置会被自动添加到 PATH）。assets 匹配时，也支持下载安装单个 `.exe` 文件或 `.msi` 文件。

bpm 在 windows 上使用类似 scoop 的 shims 机制，使用 exe 代理启动安装的软件，可以避免 cmd/sh 等兼容性问题。（原版 python 不支持此功能）
