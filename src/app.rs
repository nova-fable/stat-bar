//! 面板窗口管理：打开/关闭、定位、按视图联动高度。

use std::cell::RefCell;

use objc2::MainThreadMarker;
use objc2::ClassType;
use objc2_app_kit::{NSView, NSScreen, NSVisualEffectView};
use objc2_foundation::{NSObjectProtocol, NSRect};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Pixels, Styled, Window, WindowBounds,
    WindowKind, WindowOptions, point, px, size,
};

use crate::status_item;
use crate::ui::{PanelView, Resource, View};

/// 面板宽度
pub const PANEL_W: f32 = 360.0;
/// 各视图高度（切换视图时联动窗口高度）
pub const VIEW_HOME_H: f32 = 686.0;
pub const VIEW_SETTINGS_H: f32 = 650.0;
pub const VIEW_DETAIL_H: f32 = 686.0;
/// CPU 详情：比其他详情多两行用户态/系统态图例，趋势图也更高
pub const VIEW_CPU_DETAIL_H: f32 = 778.0;
/// 磁盘详情：蝶形 IO 曲线 + 读/写图例 + 进程列表
pub const VIEW_DISK_DETAIL_H: f32 = 720.0;

pub fn view_h(view: &View) -> f32 {
    match view {
        View::Home => VIEW_HOME_H,
        View::Settings => VIEW_SETTINGS_H,
        View::Detail(r) => match r {
            Resource::Cpu => VIEW_CPU_DETAIL_H,
            Resource::Disk => VIEW_DISK_DETAIL_H,
            _ => VIEW_DETAIL_H,
        },
    }
}

thread_local! {
    static PANEL_HANDLE: RefCell<Option<AnyWindowHandle>> = const { RefCell::new(None) };
    static PANEL_VIEW: RefCell<Option<Entity<PanelView>>> = const { RefCell::new(None) };
}

pub fn panel_handle() -> Option<AnyWindowHandle> {
    PANEL_HANDLE.with(|c| c.borrow().clone())
}

pub fn panel_view() -> Option<Entity<PanelView>> {
    PANEL_VIEW.with(|c| c.borrow().clone())
}

/// 点击状态栏：面板开着就关，关着就开（位置锚在状态栏按钮下）
pub fn toggle_panel(cx: &mut App) {
    let bounds = panel_bounds();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        kind: WindowKind::PopUp,
        titlebar: None, // HUD 无边框（Default 会带红绿灯标题栏）
        // HUD：窗口底层铺 NSVisualEffectView 毛玻璃，内容层再叠半透明底色
        window_background: gpui_kit::WindowBackgroundAppearance::Blurred,
        // HUD 固定锚在状态栏下方：禁止拖拽移动与手动调整大小（程序化 resize 不受影响）
        is_movable: false,
        is_resizable: false,
        ..Default::default()
    };

    PANEL_HANDLE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let should_open = match slot.take() {
            Some(handle) => {
                // 窗口若已被系统自动关闭，update 会报错 → 走重新打开
                handle
                    .update(cx, |_root, window, _cx| window.remove_window())
                    .is_err()
            }
            None => true,
        };
        if should_open {
            // 根视图背景透明：gpui-component 的根插件会把整窗铺满不透明主题底色，
            // 盖住毛玻璃层与面板底色，透明度因此不可见；这里用 Root 自身的样式覆盖它
            let mut built = None;
            match cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| PanelView::new(window, cx));
                built = Some(view.clone());
                cx.new(|cx| {
                    let mut root = gpui_kit::base::Root::new(view, window, cx);
                    *root.style() = gpui_kit::StyleRefinement::default().bg(gpui_kit::transparent_black());
                    root
                })
            }) {
                Ok(handle) => {
                    *slot = Some(handle.into());
                    PANEL_VIEW.with(|c| *c.borrow_mut() = built);
                }
                Err(e) => eprintln!("[statbar] 打开面板失败: {e}"),
            }
        }
    });
}

/// 关闭面板（后续通过 windowDidResignKey 或 Esc 实现）
#[allow(dead_code)]
pub fn close_panel(cx: &mut App) {
    if let Some(handle) = panel_handle() {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        PANEL_HANDLE.with(|c| *c.borrow_mut() = None);
        PANEL_VIEW.with(|c| *c.borrow_mut() = None);
    }
}

/// 面板失焦即收起（点面板外任何位置、切换应用）。
/// 点状态栏按钮时由按钮的 toggle 负责收起，这里跳过，避免先关掉又被重新打开
pub fn dismiss_on_deactivate(window: &mut Window) {
    if window.is_window_active() || status_item::pointer_on_button() {
        return;
    }
    window.remove_window();
    PANEL_HANDLE.with(|c| *c.borrow_mut() = None);
}

/// 调整面板窗口高度（macOS 27 的 setContentSize 锚定左上角，顶边保持在菜单栏下方）
pub fn set_panel_frame(window: &mut Window, h: f32) {
    window.resize(size(px(PANEL_W), px(h)));
}

/// 同步毛玻璃层透明度：gpui 的 Blurred 窗口背景会在 content view 底层插入
/// NSVisualEffectView（UnderWindowBackground 材质），该模糊层自身 alphaValue 恒为 1、
/// 近乎不透明——只调内容层 alpha 只是换色调、透不过去。这里把滑条透明度映射为
/// 模糊层 alphaValue：滑条 30%（下限）时模糊层完全透明、桌面清晰透出，
/// 100% 时全强度毛玻璃；内容层 tint 仍按 alpha 混色，整体透明度随滑条线性可感。
pub fn sync_glass_alpha(window: &mut Window, alpha: f32) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return;
    };
    // 磨砂强度：alpha 下限 0.3 → 0（无磨砂），1.0 → 1（全磨砂）
    let frost = ((alpha - 0.3) / 0.7).clamp(0.0, 1.0) as f64;
    // SAFETY: 主线程访问 AppKit 视图树；ns_view 是 gpui 的渲染视图（VIEW_CLASS），
    // 毛玻璃视图是它的同层兄弟节点——必须从父视图（窗口 contentView）开始向下找
    unsafe {
        let native_view: &NSView = &*(h.ns_view.as_ptr().cast::<NSView>());
        // Retained 先绑定再借用（临时值不能跨语句存活）
        let parent = native_view.superview();
        let start: &NSView = parent.as_deref().unwrap_or(native_view);
        set_effect_alpha(start, frost);
    }
}

/// 递归查找毛玻璃视图（NSVisualEffectView 及 gpui 的 BlurredView 子类）并设置透明度
unsafe fn set_effect_alpha(view: &NSView, alpha: f64) {
    if view.isKindOfClass(NSVisualEffectView::class()) {
        // 仅在数值变化时写入并记日志（render 每 2s 调用一次，避免刷屏）
        if (view.alphaValue() - alpha).abs() > 0.001 {
            view.setAlphaValue(alpha);
            eprintln!("[statbar] 毛玻璃层磨砂强度 → {:.2}", alpha);
        }
        return;
    }
    for sub in view.subviews().iter() {
        set_effect_alpha(&sub, alpha);
    }
}

/// 以状态栏按钮为锚点计算面板位置（gpui 坐标系原点在左上）
fn panel_bounds() -> Bounds<Pixels> {
    let mtm = MainThreadMarker::new().expect("main thread");

    status_item::with_status_button(|button| {
        let Some(button) = button else {
            return fallback_bounds(mtm);
        };
        let Some(window) = button.window() else {
            return fallback_bounds(mtm);
        };
        // 按钮所在屏幕的全局 frame（AppKit 左下原点）
        let Some(screen_frame) = window.screen().map(|s| s.frame()) else {
            return fallback_bounds(mtm);
        };
        let f: NSRect = window.frame();

        // 状态栏窗口在布局完成前会报告退化 frame（如 122x0）——此时退回屏幕右上角
        if f.size.height < 4.0 {
            eprintln!("[statbar] 状态栏窗口 frame 退化（{f:?}），使用右上角回退");
            return fallback_bounds(mtm);
        }

        // x：多屏时屏幕本地坐标 ≠ 全局坐标，x 限制在按钮所在屏幕的全局水平范围内
        let x = ((f.origin.x + f.size.width) as f32 - PANEL_W)
            .max((screen_frame.origin.x + 8.0) as f32)
            .min((screen_frame.origin.x + screen_frame.size.width - PANEL_W as f64 - 8.0) as f32);
        // y：AppKit（左下原点）→ CG/gpui（左上原点）换算，参照主屏高度（坐标系原点所在屏）
        let primary_h = primary_screen_height(mtm).unwrap_or(screen_frame.size.height);
        let y_top = primary_h - f.origin.y - f.size.height;
        eprintln!(
            "[statbar] 按钮窗口 frame=({:.0},{:.0},{:.0}x{:.0}) 屏=({:.0},{:.0} {:.0}x{:.0}) → 面板 ({:.0},{:.0})",
            f.origin.x, f.origin.y, f.size.width, f.size.height,
            screen_frame.origin.x, screen_frame.origin.y,
            screen_frame.size.width, screen_frame.size.height, x, y_top
        );
        Bounds {
            origin: point(px(x), px(y_top as f32 + 4.0)),
            size: size(px(PANEL_W), px(VIEW_HOME_H)),
        }
    })
}

/// 主屏高度：AppKit 全局坐标系的原点所在屏（frame 原点为 (0,0)）
fn primary_screen_height(mtm: MainThreadMarker) -> Option<f64> {
    NSScreen::screens(mtm)
        .iter()
        .find(|s| s.frame().origin.x == 0.0 && s.frame().origin.y == 0.0)
        .map(|s| s.frame().size.height)
}

/// 回退位置：主屏右上角、紧贴状态栏下方
fn fallback_bounds(mtm: MainThreadMarker) -> Bounds<Pixels> {
    let frame = primary_screen_frame(mtm)
        .or_else(|| NSScreen::mainScreen(mtm).map(|s| s.frame()));
    let (scr_x, scr_w) = frame
        .map(|f| (f.origin.x, f.size.width))
        .unwrap_or((0.0, 1440.0));
    Bounds {
        origin: point(px((scr_x + scr_w - PANEL_W as f64 - 8.0) as f32), px(4.0)),
        size: size(px(PANEL_W), px(VIEW_HOME_H)),
    }
}

/// 主屏 frame（AppKit 全局坐标系的原点所在屏）
fn primary_screen_frame(mtm: MainThreadMarker) -> Option<NSRect> {
    NSScreen::screens(mtm)
        .iter()
        .find(|s| s.frame().origin.x == 0.0 && s.frame().origin.y == 0.0)
        .map(|s| s.frame())
}

/// 调试/测试：延迟打开面板并可选切换到指定视图
pub fn debug_open(cx: &mut App, view: Option<View>) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let _ = cx.update(|app| {
            toggle_panel(app);
            if let Some(target) = view {
                if let Some(handle) = panel_handle() {
                    let _ = handle.update(app, |_, window, _| {
                        set_panel_frame(window, view_h(&target));
                    });
                }
                if let Some(v) = panel_view() {
                    v.update(app, |vv, _cx| vv.apply_view(target));
                }
            }
        });
    })
    .detach();
}
