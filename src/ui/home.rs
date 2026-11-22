//! 主视图：三张资源卡（点击进入对应详情视图）+ 底部操作行。

use gpui_kit::{
    App, Context, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, px, relative, rgb,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::theme::Theme;

use super::{PanelView, Resource, View};
use crate::app;
use crate::fmt::fmt_speed;
use crate::i18n::tr;
use crate::sampler::{self, Snapshot};

pub fn render(cx: &mut Context<PanelView>, theme: &Theme, s: &Snapshot) -> Div {
    let (cpu_user_data, cpu_sys_data, mem_data) = super::chart_series(&sampler::hist());
    let total_gb = s.mem_total_gb.max(0.001);

    div()
        .flex()
        .flex_col()
        .gap_3()
        // 头部：应用名
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
                            Icon::new(IconName::Activity)
                                .size(px(15.))
                                .text_color(theme.primary),
                        )
                        .child(
                            div()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child("StatBar"),
                        ),
                ),
        )
        // CPU 卡 → CPU 详情
        .child(
            card(cx, theme, "card-cpu", Resource::Cpu, tr("使用率", "Usage"))
                .child(super::big_value(theme, format!("{:.0}%", s.cpu)))
                .child(super::cpu_trend_chart(cpu_user_data, cpu_sys_data, 56.0)),
        )
        // 内存卡 → 内存详情
        .child(
            card(
                cx,
                theme,
                "card-mem",
                Resource::Memory,
                &format!(
                    "{} {:.1} / {} {:.1} GB",
                    tr("已用", "Used"), s.mem_used_gb, tr("共", "Total"), s.mem_total_gb
                ),
            )
            .child(super::big_value(theme, format!("{:.0}%", s.mem_pct)))
            // 分段堆叠条：联动 / 活跃 / 已压缩 / 实际可用
            .child(
                div()
                    .flex()
                    .w_full()
                    .h(px(10.))
                    .rounded(px(5.))
                    .overflow_hidden()
                    .bg(theme.muted)
                    .child(seg(s.wired_gb, total_gb, rgb(0x3ea6ff)))
                    .child(seg(s.active_gb, total_gb, rgb(0xff5c5c)))
                    .child(seg(s.compressed_gb, total_gb, rgb(0xa865db)))
                    .child(seg(s.avail_gb, total_gb, rgb(0x98989d))),
            )
            .child(super::legend_row(
                &theme,
                rgb(0x3ea6ff).into(),
                tr("联动", "Wired"),
                format!("{:.2} GB", s.wired_gb),
            ))
            .child(super::legend_row(
                &theme,
                rgb(0xff5c5c).into(),
                tr("活跃", "Active"),
                format!("{:.2} GB", s.active_gb),
            ))
            .child(super::legend_row(
                &theme,
                rgb(0xa865db).into(),
                tr("已压缩", "Compressed"),
                format!("{:.2} GB", s.compressed_gb),
            ))
            .child(super::legend_row(
                &theme,
                rgb(0x98989d).into(),
                tr("实际可用", "Available"),
                format!("{:.2} GB", s.avail_gb),
            ))
            .child(super::trend_chart(
                mem_data,
                rgb(0x30d158).into(),
                tr("内存", "Memory"),
                56.0,
            )),
        )
        // 底部：网络（点击进详情）与磁盘读写并列
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    card(cx, theme, "card-net", Resource::Network, tr("实时速率", "Live rate"))
                        .flex_1()
                        .child(
                            div().flex().flex_col().gap_1()
                                .child(super::net_row(
                                    &theme,
                                    IconName::ArrowUp,
                                    rgb(0xff9f0a).into(),
                                    tr("发送", "Upload"),
                                    fmt_speed(s.tx),
                                ))
                                .child(super::net_row(
                                    &theme,
                                    IconName::ArrowDown,
                                    rgb(0x64d2ff).into(),
                                    tr("接收", "Download"),
                                    fmt_speed(s.rx),
                                )),
                        ),
                )
                .child(
                    card(cx, theme, "card-disk", Resource::Disk, tr("读写", "I/O"))
                        .flex_1()
                        .child(
                            div().flex().flex_col().gap_1()
                                .child(super::net_row(
                                    &theme,
                                    IconName::ArrowDown,
                                    rgb(0xbf5af2).into(),
                                    tr("读取", "Read"),
                                    fmt_speed(s.disk_read),
                                ))
                                .child(super::net_row(
                                    &theme,
                                    IconName::ArrowUp,
                                    rgb(0xff375f).into(),
                                    tr("写入", "Write"),
                                    fmt_speed(s.disk_write),
                                )),
                        ),
                ),
        )
        // 底部操作行：设置入口 + 退出
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .child(icon_button(
                    theme,
                    "settings-btn",
                    IconName::Settings,
                    cx.listener(|this, _, window, cx| {
                        this.apply_view(View::Settings);
                        app::set_panel_frame(window, app::view_h(&View::Settings));
                        cx.notify();
                    }),
                ))
                .child(icon_button(theme, "quit-btn", IconName::Power, |_, _, cx| {
                    cx.quit();
                })),
        )
}

/// 资源卡外壳：图标 + 标题 + 说明 + 右箭头，点击进入详情视图
fn card(
    cx: &mut Context<PanelView>,
    theme: &Theme,
    id: &'static str,
    resource: Resource,
    caption: &str,
) -> Stateful<Div> {
    // 卡片底色 / 悬停底色都随面板透明度联动，避免悬停时透明度突变
    let hover_bg = super::panel_color(theme.list_hover);
    div()
        .id(id)
        .on_click(cx.listener(move |this, _, window, cx| {
            this.apply_view(View::Detail(resource));
            app::set_panel_frame(window, app::view_h(&View::Detail(resource)));
            cx.notify();
        }))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .bg(super::card_bg(theme))
        .border_1()
        .border_color(theme.border)
        .rounded(px(12.))
        .hover(move |s| s.bg(hover_bg))
        .child(
            div()
                .flex()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .items_center()
                        .child(
                            Icon::new(resource.icon())
                                .size(px(14.))
                                .text_color(resource.color()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .child(resource.title()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(caption.to_string())),
                        )
                        .child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(12.))
                                .text_color(theme.muted_foreground),
                        ),
                ),
        )
}

/// 内存分段堆叠条的一段
fn seg(gb: f64, total_gb: f64, color: gpui_kit::Rgba) -> impl IntoElement {
    let f = ((gb / total_gb).clamp(0.0, 1.0)) as f32;
    div().h(px(10.)).w(relative(f)).bg(color)
}

/// 图标按钮：悬停背景反馈，点击回调
fn icon_button<F>(theme: &Theme, id: &'static str, icon: IconName, on_click: F) -> impl IntoElement
where
    F: Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
{
    let hover_bg = theme.muted;
    div()
        .id(id)
        .flex()
        .items_center()
        .p_1()
        .rounded(px(6.))
        .text_color(theme.muted_foreground)
        .hover(move |s| s.bg(hover_bg))
        .child(Icon::new(icon).size(px(14.)))
        .on_click(on_click)
}
