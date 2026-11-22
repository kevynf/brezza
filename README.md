<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brezza-mark-white.svg">
    <source media="(prefers-color-scheme: light)" srcset="assets/brezza-mark-black.svg">
    <img src="assets/brezza-mark-black.svg" width="96" alt="brezza logo">
  </picture>
</p>

<h1 align="center">brezza</h1>

<p align="center">
  <strong>Windows 上的 Git 客户端</strong><br>
  专注提交历史、diff 与日常仓库操作。
</p>

<p align="center">
  <a href="https://github.com/kevynf/brezza/actions/workflows/ci.yml"><img src="https://github.com/kevynf/brezza/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/kevynf/brezza/releases"><img src="https://img.shields.io/github/v/release/kevynf/brezza?include_prereleases&label=release" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/kevynf/brezza" alt="License"></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D4?logo=windows&logoColor=white" alt="Windows 10 and 11">
</p>

<p align="center">简体中文 | <a href="README.en.md">English</a></p>

> [!WARNING]
> **Alpha 阶段：** brezza 仍在积极开发，功能和界面可能变化。执行重置、丢弃、强制推送等不可逆操作前，请确认目标并保留必要的备份。

你可以查看提交图、编辑工作区文件、按块暂存改动、管理分支与远程，并处理合并或变基过程中的冲突，不需要在多个工具之间来回切换。

## 功能

- **提交历史与 diff** — 提交图、提交详情与提交搜索；统一或分栏 diff；扁平或树形文件列表。
- **工作区与暂存** — 直接编辑工作区文件，暂存前自动保存未保存的改动；按文件或按块暂存、取消暂存，按文件丢弃。
- **提交** — 提交、修订上一次提交，并可删除未跟踪文件；diff 支持语法高亮。
- **历史操作** — 合并、变基、拣选、还原、重置和 bisect；冲突可标记解决，或用外部合并工具打开。
- **分支与远程** — 分支、标签、远程和储藏管理；获取、拉取、推送、强制推送和上游跟踪。
- **桌面体验** — 多标签页、命令面板；浅色、深色或跟随系统；英文与简体中文。

## 下载与运行

从 [Releases](https://github.com/kevynf/brezza/releases) 下载最新的 Windows 压缩包。压缩包内只有一个 `brezza.exe`，没有安装程序；解压后即可运行。

### 运行要求

- Windows 10 或 11（x86_64）
- [Git for Windows](https://git-scm.com/download/win)，并确保 `git` 位于 `PATH` 中。brezza 通过 Git CLI 执行写操作。
- 仅从源码构建时需要 Rust 1.95 或更高版本。

## 使用

启动后可以打开现有仓库、克隆远程仓库或初始化新仓库。每个仓库在独立标签页中打开，点击 `+` 可以新建标签页。

也可以从命令行直接打开仓库：

```powershell
.\brezza.exe C:\path\to\repository
```

按 `Ctrl+P` 打开命令面板。提交、分支、标签和储藏都提供右键菜单。

### 快捷键

| 快捷键 | 操作 |
|---|---|
| `Ctrl+O` | 打开仓库 |
| `Ctrl+P` | 命令面板 |
| `F5` | 刷新 |
| `Ctrl+S` | 保存正在编辑的工作区文件 |
| `Ctrl+Enter` | 提交 |
| `F7` / `Shift+F7` | diff 中下一个 / 上一个更改 |
| `Enter` / `Shift+Enter` | 提交搜索的下一个 / 上一个匹配 |

## 从源码构建

需要 Rust 1.95 或更高版本。

```powershell
git clone https://github.com/kevynf/brezza.git
cd brezza
cargo build --release
.\target\release\brezza.exe
```

提交改动前请运行：

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

新增或移除 `icons::*` 图标引用后，运行 `uv run scripts/subset_icons.py` 重新生成 `assets/lucide.ttf` 与 `src/ui/icons.rs`。

## 实现

- 使用 [`gix`](https://github.com/GitoxideLabs/gitoxide) 读取仓库历史和对象。
- 写操作统一通过 Git CLI 执行，行为与用户熟悉的 `git` 命令保持一致。
- 仓库读取、diff 和语法高亮在后台线程执行，避免阻塞界面。

## 反馈

遇到问题或有功能建议，请通过 [Issues](https://github.com/kevynf/brezza/issues) 反馈。

## 许可证

本项目采用 [MIT](LICENSE) 许可证。

内置的 Lucide 图标字体使用 ISC 许可证，见 [`assets/LICENSE-lucide`](assets/LICENSE-lucide)。
