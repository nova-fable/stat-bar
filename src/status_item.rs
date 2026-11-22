//! 菜单栏状态项：组合图标绘制（CoreGraphics + CoreText）、点击桥接、采样定时器。

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{AnyThread, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSCellImagePosition, NSEvent, NSFont, NSImage,
    NSStatusBar, NSStatusBarButton, NSStatusItem, NSFontWeightRegular,
};
use objc2_core_graphics::{CGContext, CGColorSpace, CGLineCap, CGBitmapContextCreateImage};
use objc2_core_foundation::CFAttributedString;
use objc2_core_text::CTLine;
use objc2_foundation::{
    NSMutableAttributedString, NSRange, NSPoint, NSRect, NSSize, NSString, NSTimer,
};

use crate::fmt::fmt_speed;
use crate::sampler;

/// NSVariableStatusItemLength：宽度随内容自适应
const VARIABLE_STATUS_ITEM_LENGTH: f64 = -1.0;

thread_local! {
    static STATUS_BUTTON: RefCell<Option<Retained<NSStatusBarButton>>> = const { RefCell::new(None) };
    /// NSStatusBar 不保留 status item 的强引用——必须自己长期持有，否则 item 立即从菜单栏消失
    static STATUS_ITEM: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
}

/// 点击事件通道：AppKit 回调发、GPUI 前台任务收。空闲时零唤醒。
static CLICK_TX: Mutex<Option<async_channel::Sender<()>>> = Mutex::new(None);

/// 注册点击事件通道（main 装配时调用一次）
pub fn set_click_channel(tx: async_channel::Sender<()>) {
    *CLICK_TX.lock().unwrap() = Some(tx);
}

/// 访问状态栏按钮（app 定位面板用）
pub(crate) fn with_status_button<R>(f: impl FnOnce(Option<&NSStatusBarButton>) -> R) -> R {
    STATUS_BUTTON.with(|cell| f(cell.borrow().as_deref()))
}

/// 鼠标是否位于状态栏按钮的屏幕范围内
pub(crate) fn pointer_on_button() -> bool {
    with_status_button(|button| {
        let Some(button) = button else { return false };
        let Some(window) = button.window() else { return false };
        let r = window.convertRectToScreen(button.convertRect_toView(button.bounds(), None));
        let p = NSEvent::mouseLocation();
        p.x >= r.origin.x
            && p.x <= r.origin.x + r.size.width
            && p.y >= r.origin.y
            && p.y <= r.origin.y + r.size.height
    })
}

define_class!(
    /// 接收状态栏按钮 action 的空 ObjC 对象，向点击通道发事件。
    #[unsafe(super(NSObject))]
    #[name = "StatBarClickTarget"]
    struct ClickTarget;

    impl ClickTarget {
        #[unsafe(method(toggle))]
        fn toggle(&self) {
            if let Some(tx) = CLICK_TX.lock().unwrap().as_ref() {
                let _ = tx.try_send(());
            }
        }
    }
);

impl ClickTarget {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

/// 经典 CGBitmapContextCreate 不在这版 objc2 绑定里（只有 Adaptive 变体），
/// 直接声明 C 符号——CoreGraphics 一直导出它。
pub(crate) mod cg_shim {
    use std::ffi::c_void;
    extern "C" {
        pub fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: *const c_void,
            bitmap_info: u32,
        ) -> *mut c_void;
    }
}

/// 菜单栏整条图标：[CPU 趋势图][内存条][网络 ↓/↑ 两行堆叠]
/// 全部黑色绘制 + setTemplate(true)，由系统按菜单栏深浅色着色。
fn status_item_image(cpu: &[f32], snap: &sampler::Snapshot) -> Option<Retained<NSImage>> {
    const H: usize = 44; // 22pt @2x
    const CPU_W: usize = 48; // 24pt @2x

    unsafe {
        // 网络文本行：8.5pt @2x → 17px；等宽数字
        let font = NSFont::monospacedDigitSystemFontOfSize_weight(17.0, NSFontWeightRegular);
        let font_any: &AnyObject = &*(Retained::as_ptr(&font) as *const AnyObject);
        let font_key = NSString::from_str("NSFont");

        let rx_text = fmt_speed(snap.rx);
        let tx_text = fmt_speed(snap.tx);
        let make_line = |text: &str| -> Retained<NSMutableAttributedString> {
            let a = NSMutableAttributedString::initWithString(
                NSMutableAttributedString::alloc(),
                &NSString::from_str(text),
            );
            a.addAttribute_value_range(
                &font_key,
                font_any,
                NSRange {
                    location: 0,
                    length: text.chars().count(),
                },
            );
            a
        };
        let line1_attr = make_line(&rx_text);
        let line2_attr = make_line(&tx_text);
        // NSAttributedString → CFAttributedString（toll-free 桥接）
        let cf1 = &*(Retained::as_ptr(&line1_attr) as *const CFAttributedString);
        let cf2 = &*(Retained::as_ptr(&line2_attr) as *const CFAttributedString);
        let line1 = CTLine::with_attributed_string(cf1);
        let line2 = CTLine::with_attributed_string(cf2);
        // typographic_bounds 返回行宽（macOS 27 无 CTLineGetWidth）
        let w1 = line1.typographic_bounds(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        let w2 = line2.typographic_bounds(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        let net_w = w1.max(w2).ceil() as usize;

        let cpu_x = 2.0;
        let mem_x = cpu_x + CPU_W as f64 + 8.0; // 段间距 4pt
        let bar_w = 10.0; // 内存条宽 5pt
        let net_x = mem_x + bar_w + 8.0;
        let total_w = (net_x + net_w as f64 + 2.0).ceil() as usize;

        let space = CGColorSpace::new_device_rgb()?;
        let mut buf = vec![0u8; total_w * H * 4];
        let ctx_ptr = cg_shim::CGBitmapContextCreate(
            buf.as_mut_ptr() as *mut c_void,
            total_w,
            H,
            8,
            total_w * 4,
            &*space as *const _ as *const c_void,
            1, // kCGImageAlphaPremultipliedLast
        );
        if ctx_ptr.is_null() {
            return None;
        }
        let ctx = &*(ctx_ptr as *const CGContext);
        CGContext::set_line_cap(Some(ctx), CGLineCap::Round);

        // ---- CPU 迷你趋势：外框 + 1pt 细线（与内存条/网速视觉对齐）----
        let (top, bottom) = (36.0, 8.0);
        let n = cpu.len() as f64;
        let step = (CPU_W as f64 - 4.0) / (n - 1.0).max(1.0);
        let y_of = |v: f32| bottom + (v as f64 / 100.0).clamp(0.0, 1.0) * (top - bottom);
        CGContext::set_rgb_stroke_color(Some(ctx), 0.0, 0.0, 0.0, 0.4);
        CGContext::set_line_width(Some(ctx), 2.0);
        CGContext::add_rect(
            Some(ctx),
            NSRect {
                origin: NSPoint { x: cpu_x, y: bottom },
                size: NSSize {
                    width: CPU_W as f64 - 2.0,
                    height: top - bottom,
                },
            },
        );
        CGContext::stroke_path(Some(ctx));
        CGContext::set_rgb_stroke_color(Some(ctx), 0.0, 0.0, 0.0, 1.0);
        for (i, v) in cpu.iter().enumerate() {
            let x = cpu_x + step * i as f64;
            if i == 0 {
                CGContext::move_to_point(Some(ctx), x, y_of(*v));
            } else {
                CGContext::add_line_to_point(Some(ctx), x, y_of(*v));
            }
        }
        CGContext::stroke_path(Some(ctx));

        // ---- 内存：纵向比例堆叠条（自下而上 可用/联动/活跃/已压缩），段间 2px 留缝 ----
        let bar_y = 6.0;
        let bar_h = 32.0;
        let usable = bar_h - 3.0 * 2.0;
        let total = snap.mem_total_gb.max(0.001);
        let segs = [
            (snap.avail_gb / total, 0.30), // 实际可用（最浅）
            (snap.wired_gb / total, 0.95), // 联动
            (snap.active_gb / total, 0.95), // 活跃
            (snap.compressed_gb / total, 0.95), // 已压缩
        ];
        let mut y = bar_y;
        for (frac, alpha) in segs {
            let h = ((frac as f64).clamp(0.0, 1.0) * usable).max(1.5);
            CGContext::set_rgb_fill_color(Some(ctx), 0.0, 0.0, 0.0, alpha);
            CGContext::fill_rect(
                Some(ctx),
                NSRect {
                    origin: NSPoint { x: mem_x, y },
                    size: NSSize {
                        width: bar_w,
                        height: h,
                    },
                },
            );
            y += h + 2.0;
        }

        // ---- 网络两行堆叠文本：右对齐（上传在上、下载在下）----
        CGContext::set_rgb_fill_color(Some(ctx), 0.0, 0.0, 0.0, 1.0);
        CGContext::set_text_position(Some(ctx), net_x + net_w as f64 - w2, 30.0);
        line2.draw(ctx);
        CGContext::set_text_position(Some(ctx), net_x + net_w as f64 - w1, 10.0);
        line1.draw(ctx);

        let img_retained = CGBitmapContextCreateImage(Some(ctx))?;
        let cg_img = &*img_retained;
        let image = NSImage::initWithCGImage_size(
            NSImage::alloc(),
            cg_img,
            NSSize {
                width: total_w as f64 / 2.0,
                height: H as f64 / 2.0,
            },
        );
        image.setTemplate(true);
        Some(image)
    }
}

/// 采样一轮并刷新菜单栏图标
fn refresh(button: &NSStatusBarButton) {
    sampler::tick();
    let snap = sampler::snapshot();
    let hist = sampler::hist();
    let cpu_hist: Vec<f32> = hist.cpu.iter().copied().collect();
    if let Some(img) = status_item_image(&cpu_hist, &snap) {
        button.setImage(Some(&img));
        button.setImagePosition(NSCellImagePosition::ImageOnly);
    }
}

/// 创建状态栏 item + 点击桥接 + 采样定时器（主装配调用一次）
pub fn setup(t0: Instant) {
    let mtm = MainThreadMarker::new().expect("StatBar 必须在主线程启动");

    let app = NSApplication::sharedApplication(mtm);
    // Accessory：不在 Dock 显示图标
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let status_item: Retained<NSStatusItem> =
        NSStatusBar::systemStatusBar().statusItemWithLength(VARIABLE_STATUS_ITEM_LENGTH);
    STATUS_ITEM.with(|slot| *slot.borrow_mut() = Some(status_item.clone()));

    let button = status_item.button(mtm).expect("状态栏 item 没有内置 button");
    // 首帧图标生成前的占位文本，ImageOnly 生效后不再显示
    button.setTitle(&NSString::from_str(" StatBar"));

    // 点击桥接：AppKit action → channel → GPUI 前台任务
    let target = ClickTarget::new();
    unsafe {
        button.setTarget(Some(&target));
        button.setAction(Some(sel!(toggle)));
    }

    // 桥接对象与按钮需长期存活
    STATUS_BUTTON.with(|slot| *slot.borrow_mut() = Some(button));
    std::mem::forget(target);

    // 主 runloop 定时器驱动采样（gpui 的事件循环同样泵主 runloop，NSTimer 照常触发）
    let button_for_tick = STATUS_BUTTON.with(|slot| slot.borrow().as_ref().unwrap().clone());
    // 立即采样一次：图标开门就有数据
    refresh(&button_for_tick);
    let tick = RcBlock::new(move |_timer: NonNull<NSTimer>| {
        refresh(&button_for_tick);
    });
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(sampler::TICK_SECS, true, &tick) };

    eprintln!("[statbar] 状态栏就绪: {:?}", t0.elapsed());
}
