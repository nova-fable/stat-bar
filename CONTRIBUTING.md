# 贡献指南

感谢你关注 StatBar。提交 Issue 或 Pull Request 前，请先阅读本文。

## 开发环境

- macOS 27+（Apple Silicon），Xcode 27+，Rust stable
- 首次构建前安装 Metal Toolchain：`xcodebuild -downloadComponent MetalToolchain`
- 构建与运行：`cargo run --release`，调试参数见 [README](README.md#构建与运行)

## 提交前检查

1. `cargo build --release --locked` 必须通过，且不要修改 `Cargo.lock`，除非确实升级了依赖
2. 改动只做必要的修改，避免顺带重排无关代码
3. 在真机上验证 UI 改动（面板 / 设置页），并说明验证方式

## 代码约定

- 分层：采样逻辑进 `sampler`，AppKit 调用进 `status_item`，窗口管理进 `app`，视图进 `ui`，不要把逻辑塞回 `main.rs`
- 注释与文档使用中文，描述软件自身的功能
- 常量集中在使用模块顶部，调试开关统一走 `--xxx` 启动参数
- 组件与主题色优先使用 gpui-kit 提供的组件与主题令牌

平台相关的踩坑记录（objc2 / CoreText / gpui 的已知问题与解决方式）见 [AGENTS.md](AGENTS.md)，修改相关代码前建议先读。

## 提交与 Pull Request

- 提交信息用一行简述改动，并说明原因（为什么改），例如 `修复面板失焦后无法收起的问题`
- 一个 PR 聚焦一件事，PR 描述中写明动机、改动范围与验证方式
- 涉及用户可见行为或发布流程的改动，请同步更新 README

## 许可

提交贡献即表示你同意你的改动以 [MIT 许可证](LICENSE) 发布。
