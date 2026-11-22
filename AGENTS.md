# AGENTS.md — StatBar 开发规范与约束

macOS 菜单栏系统监视器：状态栏常驻图标，面板实时显示 CPU / 内存 / 网速。

## 技术栈

- **UI**：gpui-kit 0.10（GPUI 生态，`gpui_kit` 单一依赖门面）+ objc2 0.6（AppKit 绑定）
- **采样**：sysinfo 0.33 + libc（host_statistics64：VM 分类内存 + CPU tick 差分；
  proc_pid_rusage：进程磁盘 IO）
  + IOKit（块设备驱动注册表统计 → 磁盘读写速率，extern shim 同 CGBitmapContextCreate）
- **目标**：macOS 27+，aarch64；常驻 HUD 应用（Accessory，无 Dock 图标）

## 目录结构

```
src/
├── main.rs          入口装配（application/主题外观/状态栏/点击桥接/调试参数）
├── app.rs           面板窗口生命周期：打开/定位/按视图联动高度
├── status_item.rs   菜单栏状态项：组合图标绘制(CG+CoreText)/点击桥接/采样定时器
├── sampler.rs       系统采样与明细发布（快照/历史/VM 分类/进程/网卡）
├── prefs.rs         设置持久化（底色/透明度/主题外观 → NSUserDefaults）
├── fmt.rs           数值格式化
└── ui/
    ├── mod.rs       PanelView 根视图 + View/Resource 状态机 + 主题三态 + 共享部件
    ├── home.rs      主视图（三张资源卡，点击进入详情）
    ├── detail.rs    资源详情视图（大图 + 明细列表）
    └── settings.rs  设置视图（主题模式/底色预设/透明度/版本/框架信息）
```

改动代码时遵守现有分层：采样逻辑进 `sampler`，AppKit 调用进 `status_item`，
窗口管理进 `app`，视图进 `ui`。不要把逻辑塞回 main.rs。

## 组件、样式、动画：优先使用 gpui-kit

**硬性约束：能用 gpui-kit 组件库实现的，必须用组件库，不要手搓近似品。**

- 主题色一律取主题令牌（`cx.theme().group_box / border / muted_foreground / popover / foreground`），
  禁止手写近似色值冒充主题色；品牌数据色（CPU 蓝 / 内存绿 / 收发青橙）例外，用 `rgb()`
- 动画优先用库内建：`Collapsible`（弹簧展开）、`Progress`（加载动画）、图表 `appear`；
  自己实现的动效需说明为什么库内没有
- 常用组件对照：开关 `Switch`、按钮 `Button` + `ButtonVariants`、图标 `Icon` +
  `assets::IconName`（Lucide 全目录）、图表 `chart::LineChart`、键帽 `Kbd`、
  分段选择 `TabBar::segmented()` + `Tab`（主题模式三态用）
- 视图切换用 `PanelView::apply_view` + `app::set_panel_frame`，不要在卡内原地展开

## 资源占用约束（常驻应用，不可妥协）

- **进程/网卡明细枚举只在详情视图打开时进行**（`sampler::ACTIVE` 控制），
  主视图绝不枚举进程
- `System::new()` 而非 `new_all()`（后者枚举全部进程，内存翻倍）
- CPU 使用率是两次刷新的差分：详情页首帧全零，`ACTIVE_TICKS >= 2` 才发布列表
- 空闲行为目标：一个 2s 定时器 + 微秒级采样 + 零 GPU/额外进程

## 平台陷阱（都是实测踩过的，别再踩）

**objc2 / AppKit**
- NSStatusBar 不保留 status item 强引用——`Retained<NSStatusItem>` 必须长期持有
- `setAttributedStringValue` 在状态栏按钮上静默不生效（macOS 27 实测）——
  菜单栏文字样式用 `button.setFont(monospacedDigit)` 实现
- 状态栏窗口在布局完成前 frame 退化（如 122x0）——定位前检查并回退
- 经典 `CGBitmapContextCreate` 不在 objc2-core-graphics 绑定里，用 extern "C" shim 声明

**CoreText**
- macOS 27 的 CoreText.tbd 不导出 `CTLineGetWidth`——行宽用
  `CTLine::typographic_bounds` 拿

**gpui / gpui-kit**
- 主题默认 **Light**：启动必须显式应用外观——走 `ui::init_theme(cx)`
  （三态：跟随系统 `Theme::sync_system_appearance` / 亮 / 暗 `Theme::change`，
  并同步 `window.observe_window_appearance` 监听系统切换）
- Blurred 窗口背景的模糊层（NSVisualEffectView，UnderWindowBackground 材质）自身
  alphaValue 恒为 1、近乎不透明：透明度调节必须同步设置模糊层 alphaValue
  （`app::sync_glass_alpha`）。注意它是 gpui 渲染视图（raw handle 的 ns_view）的
  **同层兄弟节点**，要从其 superview 开始遍历才能找到；磨砂强度映射为
  `(alpha - 0.3) / 0.7`（滑条下限时完全透明、100% 全磨砂）
- `application()` 默认不挂资产源——图标空白必须
  `application().with_assets(gpui_kit::assets::AllAssets)`
- `setContentSize` 锚定左上角：面板高度联动直接 `window.resize()` 即可，
  不要再做 origin 补偿（会双重偏移）
- Rust 死锁两种经典形态（都在本项目实测过）：
  1. render 巨型链式表达式中多处 `Mutex::lock()`——临时 guard 活到整条语句结束，
     同线程二次加锁即死锁 → 链式 UI 构建前先 clone 出数据
  2. `match *M.lock() { …分支内再 M.lock()… }`——scrutinee guard 活到 match 结束
     → 先取值绑定变量再 match
- `.id()` 来自 `InteractiveElement`、`.on_click()` 来自 `StatefulInteractiveElement`、
  `cx.new` 来自 `AppContext`——导入缺失时报"no method named"

## 调试

```sh
cargo run --release                    # 正常运行
cargo run --release -- --open-panel    # 启动即开面板
cargo run --release -- --open-cpu      # 启动即进 CPU 详情
cargo run --release -- --open-net      # 启动即进网络详情
cargo run --release -- --open-disk     # 启动即进磁盘详情
cargo run --release -- --open-settings # 启动即进设置
```

面板冻结/卡死用 `sample <pid> 2 -file out.txt` 采样定位
（主线程卡在 `Mutex::lock` = 我们的自死锁，见上）。

## 风格

- 注释与文档一律中文，描述软件自身的功能，**不出现竞品名**
- 常量集中在使用模块顶部；调试参数一律走 `--xxx` flag，不留死代码开关
