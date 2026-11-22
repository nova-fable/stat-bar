//! 设置视图：外观（主题模式 / 底色预设 / 透明度）/ 关于（版本 / 开发者 / 仓库 / 协议）/ 框架信息。

use objc2_foundation::NSProcessInfo;

use gpui_kit::{
    Context, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::slider::{Slider, SliderState};
use gpui_kit::component::tab::TabBar;
use gpui_kit::component::theme::Theme;

use super::{PanelSkin, PanelView, SKIN_DARK_END, SKIN_PRESETS, ThemeChoice, View};
use crate::app;

/// 开发者署名、项目仓库与开源协议（关于卡片展示）
const AUTHOR: &str = "dsjerry";
const REPO_URL: &str = "https://github.com/nova-fable/StatBar";
const LICENSE: &str = "MIT";

pub fn render(
    cx: &mut Context<PanelView>,
    theme: &Theme,
    slider: &gpui_kit::Entity<SliderState>,
) -> Div {
    let os = NSProcessInfo::processInfo()
        .operatingSystemVersionString()
        .to_string();
    let skin = super::panel_skin();
    let choice = super::theme_choice();
    // 底色预设按当前外观极性分组显示：暗色主题给深色组，亮色主题给亮色组
    let light_mode = !theme.mode.is_dark();
    let hover_bg = theme.muted;

    div()
        .flex()
        .flex_col()
        .gap_3()
        // 头部：返回 + 标题
        .child(
            div()
                .flex()
                .justify_between()
                .mb_2()
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .items_center()
                        .child(
                            div()
                                .id("back-btn")
                                .flex()
                                .items_center()
                                .p_1()
                                .rounded(px(6.))
                                .text_color(theme.muted_foreground)
                                .hover(move |s| s.bg(hover_bg))
                                .child(Icon::new(IconName::ChevronLeft).size(px(13.)))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.apply_view(View::Home);
                                    app::set_panel_frame(window, app::VIEW_HOME_H);
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child("设置"),
                        ),
                ),
        )
        // 外观：主题模式 + 底色预设 + 透明度
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .bg(super::card_bg(theme))
                .border_1()
                .border_color(theme.border)
                .rounded(px(12.))
                .child(section_title(theme, "外观"))
                // 主题：跟随系统 / 亮色 / 暗色（gpui-kit 分段控件）
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(div().text_color(theme.foreground).child("主题"))
                        .child(
                            TabBar::new("theme-choice")
                                .segmented()
                                .children(ThemeChoice::ALL.iter().map(|c| c.label()))
                                .selected_index(choice.index())
                                .on_click(cx.listener(|_, ix, window, cx| {
                                    super::set_theme_choice(
                                        ThemeChoice::from_index(*ix),
                                        Some(window),
                                        cx,
                                    );
                                    cx.notify();
                                })),
                        ),
                )
                // 底色：只展示与当前外观同组的预设，保证文字始终可读
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(div().text_color(theme.foreground).child("底色"))
                        .child(div().flex().gap_1().items_center().children(
                            SKIN_PRESETS
                                .iter()
                                .enumerate()
                                .filter(|(ix, _)| (*ix >= SKIN_DARK_END) == light_mode)
                                .map(|(ix, base)| swatch(cx, ix, base, &skin, theme)),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(div().text_color(theme.foreground).child("透明度"))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Slider::new(slider).w(px(140.)),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme.muted_foreground)
                                        .child(SharedString::from(format!(
                                            "{:.0}%",
                                            skin.alpha * 100.0
                                        ))),
                                ),
                        ),
                ),
        )
        // 关于：应用 / 版本 / 开发者 / 仓库 / 协议
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .bg(super::card_bg(theme))
                .border_1()
                .border_color(theme.border)
                .rounded(px(12.))
                .child(section_title(theme, "关于"))
                .child(kv_row(theme, "应用", "StatBar".into()))
                .child(kv_row(
                    theme,
                    "版本",
                    format!("v{}", env!("CARGO_PKG_VERSION")),
                ))
                .child(kv_row(theme, "开发者", AUTHOR.into()))
                .child(kv_row(theme, "仓库", REPO_URL.into()))
                .child(kv_row(theme, "协议", LICENSE.into())),
        )
        // 框架信息
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .bg(super::card_bg(theme))
                .border_1()
                .border_color(theme.border)
                .rounded(px(12.))
                .child(section_title(theme, "框架信息"))
                .child(kv_row(theme, "界面框架", "GPUI · gpui-kit 0.10".into()))
                .child(kv_row(theme, "系统采样", "sysinfo 0.33".into()))
                .child(kv_row(theme, "原生绑定", "objc2 0.6".into()))
                .child(kv_row(theme, "渲染后端", "Metal".into()))
                .child(kv_row(theme, "系统", os)),
        )
}

/// 颜色预设色块：选中用主题前景色描边，点击切换面板底色并持久化
fn swatch(
    cx: &mut Context<PanelView>,
    ix: usize,
    base: &[f32; 3],
    skin: &PanelSkin,
    theme: &Theme,
) -> impl IntoElement {
    // 拷贝为拥有的数组再进闭包（listener 回调要求 'static，局部引用不能逃逸）
    let base = *base;
    let packed = ((base[0] * 255.0) as u32) << 16
        | ((base[1] * 255.0) as u32) << 8
        | ((base[2] * 255.0) as u32);
    let color = gpui_kit::rgb(packed);
    let selected = panel_skin_base_eq(skin, &base);
    let border_color = if selected {
        theme.foreground
    } else {
        theme.border
    };
    div()
        .id(SharedString::from(format!("swatch-{ix}")))
        .on_click(cx.listener(move |_, _, _, cx| {
            crate::ui::set_panel_base(base);
            cx.notify();
        }))
        .flex()
        .justify_center()
        .items_center()
        .size(px(20.))
        .rounded(px(5.))
        .bg(color)
        .border_1()
        .border_color(border_color)
}

/// 面板底色预设是否匹配当前皮肤
fn panel_skin_base_eq(skin: &PanelSkin, base: &[f32; 3]) -> bool {
    skin.base
        .iter()
        .zip(base.iter())
        .all(|(a, b)| (a - b).abs() < 0.004)
}

fn section_title(theme: &Theme, title: &'static str) -> impl IntoElement {
    div()
        .text_size(px(11.))
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .child(title)
}

fn kv_row(theme: &Theme, label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .child(div().text_color(theme.muted_foreground).child(label))
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.foreground)
                .child(SharedString::from(value)),
        )
}
