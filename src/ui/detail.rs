//! 资源详情视图：CPU / 内存 / 网络三套布局，含趋势图与可滚动明细列表。

use gpui_kit::{
    Context, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, uniform_list, div, px, relative, rgb,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::theme::Theme;

use super::{PanelView, Resource, View};
use crate::app;
use crate::fmt::fmt_speed;
use crate::i18n::tr;
use crate::icons;
use crate::sampler::{self, Snapshot};

pub fn render(
    cx: &mut Context<PanelView>,
    theme: &Theme,
    resource: Resource,
    s: &Snapshot,
) -> Div {
    let hover_bg = theme.muted;
    let rows = sampler::detail_rows();
    let (cpu_user_data, cpu_sys_data, mem_data) = super::chart_series(&sampler::hist());

    let mut root = div()
        .flex()
        .flex_col()
        .gap_3()
        // 头部：返回 + 资源名
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
                                .child(resource.title()),
                        ),
                ),
        );

    match resource {
        Resource::Cpu => {
            root = root
                .child(
                    card_shell(theme)
                        .child(caption(theme, tr("使用率", "Usage")))
                        .child(super::big_value(theme, format!("{:.0}%", s.cpu)))
                        // 用户态 / 系统态图例（配色与折线一致，参考系统活动监视器）
                        .child(super::legend_row(
                            &theme,
                            rgb(0x0a84ff).into(),
                            tr("用户", "User"),
                            format!("{:.1}%", s.cpu_user),
                        ))
                        .child(super::legend_row(
                            &theme,
                            rgb(0xff453a).into(),
                            tr("系统", "System"),
                            format!("{:.1}%", s.cpu_sys),
                        ))
                        .child(super::cpu_trend_chart(
                            cpu_user_data,
                            cpu_sys_data,
                            160.0,
                        )),
                )
                .child(list_card(
                    theme,
                    tr("进程 Top · CPU 占用", "Top processes · CPU"),
                    Resource::Cpu,
                    rows,
                ));
        }
        Resource::Memory => {
            let total_gb = s.mem_total_gb.max(0.001);
            root = root
                .child(
                    card_shell(theme)
                        .child(caption(theme, tr("使用率", "Usage")))
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
                            120.0,
                        )),
                )
                .child(list_card(
                    theme,
                    tr("进程 Top · 内存占用", "Top processes · Memory"),
                    Resource::Memory,
                    rows,
                ));
        }
        Resource::Network => {
            root = root
                .child(
                    card_shell(theme)
                        .child(caption(theme, tr("实时速率", "Live rate")))
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
                )
                .child(list_card(
                    theme,
                    tr("应用网络占用", "Per-app network"),
                    Resource::Network,
                    rows,
                ));
        }
        Resource::Disk => {
            let (disk_r, disk_w) = super::disk_series(&sampler::hist());
            root = root
                .child(
                    card_shell(theme)
                        .child(caption(theme, tr("IO 速率", "I/O rate")))
                        .child(super::disk_io_chart(disk_r, disk_w, 140.0))
                        .child(super::legend_row(
                            &theme,
                            rgb(0xbf5af2).into(),
                            tr("读取", "Read"),
                            fmt_speed(s.disk_read),
                        ))
                        .child(super::legend_row(
                            &theme,
                            rgb(0xff375f).into(),
                            tr("写入", "Write"),
                            fmt_speed(s.disk_write),
                        )),
                )
                .child(list_card(
                    theme,
                    tr("进程 Top · 磁盘 IO", "Top processes · Disk I/O"),
                    Resource::Disk,
                    rows,
                ));
        }
    }

    root
}

/// 明细列表卡：标题 + 虚拟化滚动列表（gpui uniform_list，原生滚轮，只渲染可见行）
fn list_card(
    theme: &Theme,
    title: &'static str,
    resource: Resource,
    rows: Vec<sampler::DetailRow>,
) -> Div {
    // 各资源详情页的列表可视高度（显式像素，确定性布局）
    let list_h = match resource {
        Resource::Cpu => 320.0,
        Resource::Memory => 250.0,
        Resource::Network => 430.0,
        Resource::Disk => 340.0,
    };
    let fg = theme.foreground;
    let muted = theme.muted_foreground;
    let rows_for_list = rows.clone();

    let mut card = card_shell(theme).child(caption(theme, title));

    if rows.is_empty() {
        // gpui-kit Progress：loading 模式 = 无限循环加载动画
        card = card.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .py_2()
                .child(
                    gpui_kit::component::progress::Progress::new("detail-loading")
                        .loading(true),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.muted_foreground)
                        .child(tr("正在统计…", "Collecting…")),
                ),
        );
    } else {
        card = card.child(
            uniform_list("detail-rows", rows.len(), move |range, _window, _cx| {
                rows_for_list[range]
                    .iter()
                    .map(|row| proc_row(fg, muted, row))
                    .collect::<Vec<_>>()
            })
            .h(px(list_h)),
        );
    }

    card
}

/// 单行：图标 + 名称 + 右对齐数值（uniform_list 要求行高一致）
fn proc_row(fg: gpui_kit::Hsla, muted: gpui_kit::Hsla, row: &sampler::DetailRow) -> Div {
    // 图标按 pid 缓存、渲染时惰性提取（主线程，NSWorkspace 要求）
    let icon = row.pid.and_then(|pid| icons::app_icon(pid, row.exe.as_ref()));
    let icon_el: Div = match icon {
        Some(image) => div().child(
            gpui_kit::img(gpui_kit::ImageSource::Render(std::sync::Arc::clone(
                &image,
            )))
                .size(px(14.))
                .rounded(px(3.)),
        ),
        None => div().child(
            Icon::new(IconName::AppWindow)
                .size(px(12.))
                .text_color(muted),
        ),
    };
    div()
        .flex()
        .justify_between()
        .gap_2()
        .items_center()
        .h(px(24.))
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(icon_el)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(fg)
                        .child(SharedString::from(row.name.clone())),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(fg)
                .child(SharedString::from(row.value.clone())),
        )
}

/// 卡片外壳
fn card_shell(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .bg(super::card_bg(theme))
        .border_1()
        .border_color(theme.border)
        .rounded(px(12.))
}

/// 说明文字
fn caption(theme: &Theme, text: &'static str) -> impl IntoElement {
    div()
        .text_size(px(11.))
        .text_color(theme.muted_foreground)
        .child(text)
}

/// 内存分段条的一段
fn seg(gb: f64, total_gb: f64, color: gpui_kit::Rgba) -> impl IntoElement {
    let f = ((gb / total_gb).clamp(0.0, 1.0)) as f32;
    div().h(px(10.)).w(relative(f)).bg(color)
}
