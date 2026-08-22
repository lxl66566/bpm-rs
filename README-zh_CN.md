# bpm-rs

[English](./README.md) | 简体中文

前身为 [Python 版本的 bin-package-manager](https://github.com/lxl66566/bpm)，此为其 rust 重写版本与活跃开发版本，兼容原版配置与数据存储，可直接从 Python 版无缝切换。

bpm (bin package manager) 是一个基于 Github Release 的包管理器，允许用户安装并管理任意 Github Release 上的二进制文件。支持 General Linux 与 Windows。

> [!CAUTION]
> 风险提示：bpm Linux 版存在潜在的破坏计算机风险。使用 bpm 安装软件即代表您已接受此风险，并信任第三方 Github Release 的打包者。

> [!TIP]
> bpm 保证对偶性，即 `bpm install` 后立即接 `bpm uninstall` 不会对系统作出任何改变。

### 对比

- 与 scoop/pacman 等需要打包者维护打包脚本的工具不同，bpm 全自动化，无需额外打包维护即可兼容 ~95% 的 Github Release。
- 与 cargo-binstall（仅限 Rust/crates.io）不同，bpm 不限语言，内置仓库搜索与自动 asset 选择。安装 Rust 应用成功率接近 100%。

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
bpm i <package>         # 安装（支持 GitHub 搜索、user/repo、URL）
bpm i <package> --local <压缩包|目录>   # 从本地文件安装
bpm r <package>         # 卸载
bpm u [package]         # 更新一个 / 全部
bpm list                # 查看已安装的包（--json、--outdated）
bpm doctor              # 校验已安装文件
bpm -h                  # 查看更多帮助
```

## 配置

可选配置文件位于 `~/.config/bpm/config.toml`：

```toml
# install_position = "D:/bpm"      # app/bin 根目录（db 随之移动）
# db_path = "D:/bpm/db.json"       # 单独指定 db 位置
# prefix = "/usr/local"            # unix 安装前缀（--prefix 优先）
# github_token = "ghp_xxx"         # 提升 API 速率限制；GITHUB_TOKEN/GH_TOKEN 环境变量优先
```

## 原理解释

- **选择 asset**：bpm 基于平台/架构/libc 关键词自动匹配最佳 asset；可用 `--interactive` 手动选择。
- **binary 匹配逻辑**：解压后按顺序查找：(1) 压缩包内只有一个文件时直接使用；(2) 否则递归匹配 `bin_name`（默认为包名，`--bin-name` / `-b` 覆盖，Windows 自动补 `.exe` 后缀）；(3) Windows 上未匹配到时回退使用所有 `.exe` 文件。`--one-bin` 强制单文件安装（仅 Linux）。

### Linux

bpm 自动判断 asset 中的文件结构，并安装到系统中的相应位置：

1. 安装 binary 到 `/usr/bin`
2. 合并 `lib/`, `include/`, `share/`, `man/`, `bin/` 目录到系统 `usr/` 下（如果存在）
3. 安装 completions
4. 安装 services（基于 systemd 的系统）

已存在的文件会被重命名为 `<name>.old` 而非覆盖，卸载时恢复。

由于依赖 [FHS](https://en.wikipedia.org/wiki/Filesystem_Hierarchy_Standard)，bpm 不适用于 NixOS 等不遵循 FHS 的发行版。

### Windows

bpm 将内容解压到 `%userprofile%/bpm/app/<name>`，并在 `%userprofile%/bpm/bin`（自动加入 PATH）中创建类似 scoop 的 shims 来启动可执行文件，避免 cmd/sh 兼容性问题。也支持安装单个 `.exe` 或 `.msi`。
