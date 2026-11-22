//! StatBar — macOS 菜单栏系统监视器：状态栏常驻，实时显示 CPU / 内存 / 网速。
//!
//! 模块划分：
//! - `status_item`：菜单栏状态项（组合图标绘制、点击桥接、采样定时器）
//! - `sampler`：系统采样与明细数据发布
//! - `app`：面板窗口生命周期（打开 / 定位 / 按视图联动高度）
//! - `ui`：面板视图（主页 / 资源详情 / 设置）与主题外观三态
//! - `prefs`：设置持久化（底色 / 透明度 / 主题外观选择）
//!
//! 运行：cargo run --release
//! 调试：--open-panel / --open-settings / --open-cpu / --open-net

mod app;
mod fmt;
mod icons;
mod prefs;
mod sampler;
mod status_item;
mod ui;

use std::time::Instant;

use gpui_kit::App;

use ui::{Resource, View};

fn main() {
    let t0 = Instant::now();
    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    eprintln!("[statbar] platform 初始化: {:?}", t0.elapsed());

    app.run(move |cx: &mut App| {
        eprintln!("[statbar] gpui run 回调: {:?}", t0.elapsed());
        gpui_kit::init(cx);
        // 主题外观：恢复持久化的选择并应用（默认跟随系统，亮 / 暗可在设置切换）
        ui::init_theme(cx);
        status_item::setup(t0);

        // 桥接循环：收到状态栏点击事件 → 在 GPUI 主线程切换面板。空闲时零唤醒。
        let (tx, rx) = async_channel::unbounded();
        status_item::set_click_channel(tx);
        cx.spawn(async move |cx| {
            while let Ok(()) = rx.recv().await {
                let _ = cx.update(|app| app::toggle_panel(app));
            }
        })
        .detach();

        // 调试：延迟打开面板并可选切换到指定视图
        let args: Vec<String> = std::env::args().collect();
        let want = |name: &str| args.iter().any(|a| a == name);
        let target = if want("--open-cpu") {
            Some(View::Detail(Resource::Cpu))
        } else if want("--open-net") {
            Some(View::Detail(Resource::Network))
        } else if want("--open-disk") {
            Some(View::Detail(Resource::Disk))
        } else if want("--open-settings") {
            Some(View::Settings)
        } else {
            None
        };
        if want("--open-panel") || target.is_some() {
            app::debug_open(cx, target);
        }
    });
}
