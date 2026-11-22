//! 进程应用图标提取：NSWorkspace 图标 → CGImage → RGBA → gpui RenderImage。
//! 按 pid 缓存，仅在主线程调用（NSWorkspace/NSImage 要求）。

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::RenderImage;
use objc2::rc::Retained;
use objc2_app_kit::{NSImage, NSWorkspace};
use objc2_core_graphics::{CGColorSpace, CGContext, CGLineCap};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

fn path_str(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}
use image::{Frame as ImageFrame, RgbaImage};
use smallvec::smallvec;

use crate::status_item::cg_shim;

/// 图标缓存：pid → 图标（None 表示提取失败，避免反复尝试）
static ICON_CACHE: std::sync::LazyLock<Mutex<HashMap<u32, Option<Arc<RenderImage>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

const ICON_PX: usize = 64; // 32pt @2x

/// 取进程图标（缓存命中直接返回）
pub fn app_icon(pid: u32, exe: Option<&PathBuf>) -> Option<Arc<RenderImage>> {
    if let Some(hit) = ICON_CACHE.lock().unwrap().get(&pid) {
        return hit.clone();
    }
    let icon = extract(pid, exe);
    let mut cache = ICON_CACHE.lock().unwrap();
    // 防御性上限：缓存过大时整体清空（进程集合变化很快的场景）
    if cache.len() > 512 {
        cache.clear();
    }
    cache.insert(pid, icon.clone());
    icon
}

/// 从可执行文件路径向上找最外层 .app 包
/// （Electron 应用的 helper 嵌在嵌套 .app 里，取外层主应用包才是用户认识的彩色 logo）
fn app_bundle_of(exe: &PathBuf) -> Option<PathBuf> {
    let mut found: Option<PathBuf> = None;
    for anc in exe.ancestors() {
        if anc
            .to_string_lossy()
            .to_lowercase()
            .ends_with(".app")
        {
            found = Some(anc.to_path_buf()); // 持续覆盖 → 最外层
        }
    }
    found
}

fn extract(pid: u32, exe: Option<&PathBuf>) -> Option<Arc<RenderImage>> {
    let _ = pid;
    let _mtm = objc2::MainThreadMarker::new()?;
    unsafe {
        let ws = NSWorkspace::sharedWorkspace();
        // 优先用 .app 包图标（用户认识的应用 logo），CLI 进程回退到可执行文件图标
        let icon_source = match exe {
            Some(exe_path) => app_bundle_of(exe_path).unwrap_or_else(|| exe_path.clone()),
            None => return None,
        };
        let ns_img: Retained<NSImage> = ws.iconForFile(&NSString::from_str(&path_str(&icon_source)));
        eprintln!("[statbar] icon: pid {} 源={} 图标尺寸={:?}", pid, path_str(&icon_source), ns_img.size());

        // NSImage → CGImage（32pt 见方）
        let mut proposed = ns_rect_default();
        let cg_retained = ns_img.CGImageForProposedRect_context_hints(
            &mut proposed,
            None,
            None,
        );
        eprintln!("[statbar] icon: CGImage 提取={:?}", cg_retained.is_some());
        let cg_retained = cg_retained?;
        let cg = &*cg_retained;

        // 画进位图上下文（翻转 Y 使图标正立）
        let (w, h) = (ICON_PX, ICON_PX);
        let space = CGColorSpace::new_device_rgb()?;
        let mut buf = vec![0u8; w * h * 4];
        let ctx_ptr = cg_shim::CGBitmapContextCreate(
            buf.as_mut_ptr() as *mut c_void,
            w,
            h,
            8,
            w * 4,
            &*space as *const _ as *const c_void,
            1, // kCGImageAlphaPremultipliedLast
        );
        if ctx_ptr.is_null() {
            return None;
        }
        let ctx = &*(ctx_ptr as *const CGContext);
        CGContext::set_line_cap(Some(ctx), CGLineCap::Round);
        CGContext::translate_ctm(Some(ctx), 0.0, h as f64);
        CGContext::scale_ctm(Some(ctx), 1.0, -1.0);
        let rect = NSRect {
            origin: NSPoint { x: 2.0, y: 2.0 },
            size: NSSize {
                width: (w - 4) as f64,
                height: (h - 4) as f64,
            },
        };
        CGContext::draw_image(Some(ctx), rect, Some(cg));

        // 预乘 alpha → 直通 alpha（image crate 需要）
        for px in buf.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a > 0 {
                px[0] = ((px[0] as u32 * 255) / a) as u8;
                px[1] = ((px[1] as u32 * 255) / a) as u8;
                px[2] = ((px[2] as u32 * 255) / a) as u8;
            }
        }

        let rgba = RgbaImage::from_raw(w as u32, h as u32, buf)?;
        let frame = ImageFrame::new(rgba);
        Some(Arc::new(RenderImage::new(smallvec![frame])))
    }
}

fn ns_rect_default() -> NSRect {
    NSRect {
        origin: NSPoint { x: 0.0, y: 0.0 },
        size: NSSize {
            width: 0.0,
            height: 0.0,
        },
    }
}
