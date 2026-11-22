//! 界面国际化：中文 / 英文。默认跟随系统首选语言（非中文一律回退英文），设置视图可手动切换。
//! 文案直接写成 `tr("中文", "English")`，运行时按当前语言返回对应字符串。

use std::sync::{Mutex, OnceLock};

use objc2_foundation::NSLocale;

use crate::prefs;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// 语言选择：跟随系统 / 中文 / English（设置视图可切换）
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LangChoice {
    System,
    Zh,
    En,
}

impl LangChoice {
    /// 段选控件的展示顺序
    pub const ALL: [LangChoice; 3] = [Self::System, Self::Zh, Self::En];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => tr("跟随系统", "System"),
            Self::Zh => "中文",
            Self::En => "English",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap()
    }

    pub fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(Self::System)
    }
}

static CHOICE: Mutex<LangChoice> = Mutex::new(LangChoice::System);

pub fn choice() -> LangChoice {
    *CHOICE.lock().unwrap()
}

/// 切换语言并持久化（设置视图调用）
pub fn set_choice(choice: LangChoice) {
    *CHOICE.lock().unwrap() = choice;
    prefs::save_lang_choice(choice);
}

/// 启动装配：恢复持久化的语言选择（未持久化过默认跟随系统）
pub fn init() {
    if let Some(choice) = prefs::load_lang_choice() {
        *CHOICE.lock().unwrap() = choice;
    }
}

/// 当前生效的语言：跟随系统时取系统首选语言
pub fn lang() -> Lang {
    match choice() {
        LangChoice::Zh => Lang::Zh,
        LangChoice::En => Lang::En,
        LangChoice::System => system_lang(),
    }
}

/// 系统首选语言只在首次取用时读取一次（NSLocale 调用不便频繁进行）
fn system_lang() -> Lang {
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    *SYSTEM.get_or_init(|| {
        let first = NSLocale::preferredLanguages()
            .firstObject()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if first.starts_with("zh") {
            Lang::Zh
        } else {
            Lang::En
        }
    })
}

/// 按当前语言选取文案
pub fn tr(zh: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Zh => zh,
        Lang::En => en,
    }
}
