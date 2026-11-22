//! 设置持久化：底色 / 透明度 / 主题外观选择写入 NSUserDefaults，重启后恢复。

use objc2_foundation::{NSUserDefaults, NSString, NSInteger};

use crate::i18n::LangChoice;
use crate::ui::{PanelSkin, ThemeChoice};

/// 底色（0xRRGGBB 打包整数）
const KEY_SKIN_BASE: &str = "skinBase";
/// 透明度（0.3..1.0）
const KEY_SKIN_ALPHA: &str = "skinAlpha";
/// 主题外观选择（0 跟随系统 / 1 亮色 / 2 暗色）
const KEY_THEME_CHOICE: &str = "themeChoice";
/// 语言选择（0 跟随系统 / 1 中文 / 2 English）
const KEY_LANG_CHOICE: &str = "langChoice";

/// 启动时读回底色与透明度；键缺失时返回 None（保持内置默认）
pub(crate) fn load_skin() -> Option<PanelSkin> {
    let d = NSUserDefaults::standardUserDefaults();
    let base_key = NSString::from_str(KEY_SKIN_BASE);
    let alpha_key = NSString::from_str(KEY_SKIN_ALPHA);
    if d.objectForKey(&base_key).is_none() || d.objectForKey(&alpha_key).is_none() {
        return None;
    }
    let base = unpack_base(d.integerForKey(&base_key));
    let alpha = (d.doubleForKey(&alpha_key) as f32).clamp(0.3, 1.0);
    Some(PanelSkin { base, alpha })
}

/// 启动时读回主题外观选择；键缺失或值非法返回 None
pub(crate) fn load_theme_choice() -> Option<ThemeChoice> {
    let d = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str(KEY_THEME_CHOICE);
    if d.objectForKey(&key).is_none() {
        return None;
    }
    match d.integerForKey(&key) {
        0 => Some(ThemeChoice::System),
        1 => Some(ThemeChoice::Light),
        2 => Some(ThemeChoice::Dark),
        _ => None,
    }
}

/// 启动时读回语言选择；键缺失或值非法返回 None
pub(crate) fn load_lang_choice() -> Option<LangChoice> {
    let d = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str(KEY_LANG_CHOICE);
    if d.objectForKey(&key).is_none() {
        return None;
    }
    match d.integerForKey(&key) {
        0 => Some(LangChoice::System),
        1 => Some(LangChoice::Zh),
        2 => Some(LangChoice::En),
        _ => None,
    }
}

/// 保存语言选择（设置切换时调用）
pub(crate) fn save_lang_choice(choice: LangChoice) {
    let index: NSInteger = match choice {
        LangChoice::System => 0,
        LangChoice::Zh => 1,
        LangChoice::En => 2,
    };
    NSUserDefaults::standardUserDefaults().setInteger_forKey(index, &NSString::from_str(KEY_LANG_CHOICE));
}

/// 保存底色与透明度（色块点击 / 滑条拖动时调用）
pub(crate) fn save_skin(skin: PanelSkin) {
    let d = NSUserDefaults::standardUserDefaults();
    d.setInteger_forKey(pack_base(skin.base), &NSString::from_str(KEY_SKIN_BASE));
    d.setDouble_forKey(skin.alpha as f64, &NSString::from_str(KEY_SKIN_ALPHA));
}

/// 保存主题外观选择（设置切换时调用）
pub(crate) fn save_theme_choice(choice: ThemeChoice) {
    let index: NSInteger = match choice {
        ThemeChoice::System => 0,
        ThemeChoice::Light => 1,
        ThemeChoice::Dark => 2,
    };
    NSUserDefaults::standardUserDefaults().setInteger_forKey(index, &NSString::from_str(KEY_THEME_CHOICE));
}

/// 底色 RGB 0..1 → 0xRRGGBB
fn pack_base(base: [f32; 3]) -> NSInteger {
    let ch = |v: f32| ((v * 255.0).round().clamp(0.0, 255.0)) as NSInteger;
    (ch(base[0]) << 16) | (ch(base[1]) << 8) | ch(base[2])
}

/// 0xRRGGBB → 底色 RGB 0..1
fn unpack_base(v: NSInteger) -> [f32; 3] {
    [
        ((v >> 16) & 0xff) as f32 / 255.0,
        ((v >> 8) & 0xff) as f32 / 255.0,
        (v & 0xff) as f32 / 255.0,
    ]
}
