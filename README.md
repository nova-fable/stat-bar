# StatBar

**中文** | [English](README.en.md)

[![CI](https://github.com/nova-fable/stat-bar/actions/workflows/ci.yml/badge.svg)](https://github.com/nova-fable/stat-bar/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

macOS 菜单栏系统监视器：状态栏常驻显示 CPU / 内存 / 网速，点击弹出面板查看实时数据与明细。

## 功能

- **状态栏**：常驻组合图标，实时显示 CPU 与网速
- **面板**：CPU、内存、网络、磁盘卡片，点击进入详情视图（大图趋势、内存分类、进程与网卡明细）
- **设置**：主题跟随系统 / 亮色 / 暗色，语言跟随系统 / 中文 / English，面板底色与透明度可调，设置持久化
- **低占用**：主视图不枚举进程；进程与网卡明细仅在详情视图打开时采样

## 技术路线

objc2 / AppKit 负责常驻状态栏，GPUI（经 gpui-kit）负责点击弹出的面板。两者在同一进程中运行：GPUI 接管主循环，状态栏在其 run 回调中创建，点击事件经 async-channel 桥接到 GPUI，空闲时零轮询。

实测（release，vmmap Physical footprint）：

| 状态 | 私有占用 | 启动 |
|---|---|---|
| 面板关闭（常态） | 约 16 MB | 状态栏就绪约 160 ms |
| 面板打开 | 约 31 MB（shader 加载瞬时峰值约 64 MB） | 首次打开约 390 ms（一次性） |

## 安装

从 [Releases](https://github.com/nova-fable/stat-bar/releases) 下载最新的 `StatBar-vX.Y.Z-macos-arm64.dmg`，打开后将 StatBar 拖入 Applications。

当前版本仅做 ad-hoc 签名、未经 Apple 公证，首次打开时 macOS 会提示无法验证开发者。可以在 Finder 中右键 StatBar 选择「打开」；若仍被拦截，前往「系统设置 → 隐私与安全性」，点击「仍要打开」。

## 环境要求

- macOS 27+，Apple Silicon（aarch64）
- Rust 工具链（stable）
- Xcode 27+ 及 **Metal Toolchain**（gpui 编译期需要编译 shader）：

  ```sh
  xcodebuild -runFirstLaunch
  xcodebuild -downloadComponent MetalToolchain
  ```

## 构建与运行

```sh
cargo run --release
```

左键点击状态栏图标即可开关面板。

打包 `.app` 与 DMG（需先将图标与背景图放入 `assets/`）：

```sh
scripts/bundle.sh                        # 生成 target/bundle/StatBar.app
scripts/make-dmg.sh StatBar.dmg          # 生成拖拽安装的 DMG
```

调试参数（启动即进入指定视图，便于开发时直接测试）：

| 参数 | 作用 |
|---|---|
| `--open-panel` | 启动即打开面板 |
| `--open-cpu` | 启动即进入 CPU 详情 |
| `--open-net` | 启动即进入网络详情 |
| `--open-disk` | 启动即进入磁盘详情 |
| `--open-settings` | 启动即进入设置 |

## 项目结构

```text
src/
├── main.rs          入口装配（应用、主题、状态栏、点击桥接、调试参数）
├── app.rs           面板窗口生命周期：打开 / 定位 / 按视图联动高度 / 失焦收起
├── status_item.rs   菜单栏状态项：组合图标绘制（CoreGraphics + CoreText）、点击桥接、采样定时器
├── sampler.rs       系统采样与明细发布（快照 / 历史 / 内存分类 / 进程 / 网卡 / 磁盘）
├── i18n.rs          界面国际化：中文 / 英文，默认跟随系统语言
├── prefs.rs         设置持久化（底色 / 透明度 / 主题外观 / 语言，写入 NSUserDefaults）
├── fmt.rs           数值格式化
├── icons.rs         进程应用图标提取（用于进程明细）
└── ui/              面板视图：主页、资源详情、设置与主题三态
```

`scripts/` 为打包脚本（`bundle.sh` 生成 `.app`，`make-dmg.sh` 生成 DMG），`assets/` 存放应用图标与 DMG 背景图。

## 贡献

欢迎提交 Issue 与 Pull Request。开发约定、代码分层与平台注意事项见 [CONTRIBUTING.md](CONTRIBUTING.md) 与 [AGENTS.md](AGENTS.md)。

## 路线图

- 打包 `.app`（LSUIElement）、SMAppService 开机自启、签名与公证分发
- 打磨：鼠标按下即开、右键菜单、状态栏迷你图

## 许可证

[MIT](LICENSE) © 2026 dsjerry
