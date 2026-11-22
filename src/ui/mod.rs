//! 面板 UI：视图切换（主页 / 设置 / 资源详情）+ 共享部件。

mod detail;
mod home;
mod settings;

use std::time::Duration;

use gpui_kit::{
    App, AppContext, Context, Entity, FocusHandle, FontWeight, Hsla, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, PathBuilder, Pixels, Point, Render, SharedString,
    Styled, Subscription, Window, canvas, div, point, px, rgb,
};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};

use gpui_kit::component::Icon;
use gpui_kit::component::chart::AreaChart;
use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::assets::IconName;

use crate::app;
use crate::i18n::tr;
use crate::sampler;
use crate::sampler::Hist;

/// 面板视图
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Home,
    Settings,
    Detail(Resource),
}

/// 资源种类（与采样模块的 ACTIVE 编号一一对应）
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Cpu,
    Memory,
    Network,
    Disk,
}

impl Resource {
    pub fn kind(self) -> u8 {
        match self {
            Self::Cpu => 0,
            Self::Memory => 1,
            Self::Network => 2,
            Self::Disk => 3,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => tr("内存", "Memory"),
            Self::Network => tr("网络", "Network"),
            Self::Disk => tr("磁盘", "Disk"),
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::Cpu => IconName::Cpu,
            Self::Memory => IconName::MemoryStick,
            Self::Network => IconName::Wifi,
            Self::Disk => IconName::HardDrive,
        }
    }

    /// 主页卡片强调色
    pub fn color(self) -> Hsla {
        match self {
            Self::Cpu => rgb(0x0a84ff).into(),
            Self::Memory => rgb(0x30d158).into(),
            Self::Network => rgb(0x64d2ff).into(),
            Self::Disk => rgb(0xbf5af2).into(),
        }
    }
}

/// 面板皮肤：底色（RGB 0..1）+ 透明度（0..1），设置视图可调
#[derive(Clone, Copy, PartialEq)]
pub struct PanelSkin {
    pub base: [f32; 3],
    pub alpha: f32,
}

/// 底色预设：前 4 个深色组（石墨 / 曜石 / 深蓝 / 暖棕，暗色主题显示），
/// 后 4 个亮色组（雾白 / 云灰 / 浅蓝 / 米杏，亮色主题显示）
pub(crate) const SKIN_PRESETS: [[f32; 3]; 8] = [
    [0.063, 0.063, 0.078],
    [0.0, 0.0, 0.0],
    [0.039, 0.055, 0.11],
    [0.102, 0.078, 0.063],
    [0.937, 0.941, 0.957],
    [0.855, 0.863, 0.878],
    [0.835, 0.886, 0.945],
    [0.961, 0.925, 0.863],
];

/// 深色 / 亮色两组预设的分界下标（[0, SKIN_DARK_END) 深色组，其余亮色组）
pub(crate) const SKIN_DARK_END: usize = 4;

static PANEL_SKIN: std::sync::Mutex<PanelSkin> = std::sync::Mutex::new(PanelSkin {
    base: SKIN_PRESETS[0],
    alpha: 0.82,
});

pub(crate) fn panel_skin() -> PanelSkin {
    *PANEL_SKIN.lock().unwrap()
}

pub(crate) fn set_panel_base(base: [f32; 3]) {
    let snapshot = {
        let mut skin = PANEL_SKIN.lock().unwrap();
        skin.base = base;
        *skin
    };
    crate::prefs::save_skin(snapshot);
}

pub(crate) fn set_panel_alpha(alpha: f32) {
    let snapshot = {
        let mut skin = PANEL_SKIN.lock().unwrap();
        skin.alpha = alpha.clamp(0.3, 1.0);
        *skin
    };
    crate::prefs::save_skin(snapshot);
}

/// 底色亮度（Rec.709 加权）：区分亮 / 深色两组预设的极性
fn skin_is_light(base: &[f32; 3]) -> bool {
    0.2126 * base[0] + 0.7152 * base[1] + 0.0722 * base[2] > 0.5
}

/// 底色与主题极性对齐：主题亮暗切换后，底色若属于另一组则切到该组默认色
/// （避免亮色主题配深色底色导致边缘文字不可读）
fn reconcile_skin(cx: &App) {
    let want_light = !cx.theme().mode.is_dark();
    let skin = panel_skin();
    if skin_is_light(&skin.base) != want_light {
        set_panel_base(SKIN_PRESETS[if want_light { SKIN_DARK_END } else { 0 }]);
    }
}

/// 主题外观选择：跟随系统 / 亮色 / 暗色
/// （gpui-kit 的 ThemeMode 只有亮暗两态，"跟随系统"由本层三态 + 外观监听实现）
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    /// 段选控件的展示顺序
    pub const ALL: [ThemeChoice; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => tr("跟随系统", "System"),
            Self::Light => tr("亮色", "Light"),
            Self::Dark => tr("暗色", "Dark"),
        }
    }

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap()
    }

    pub(crate) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(Self::System)
    }
}

static THEME_CHOICE: std::sync::Mutex<ThemeChoice> = std::sync::Mutex::new(ThemeChoice::System);

pub(crate) fn theme_choice() -> ThemeChoice {
    *THEME_CHOICE.lock().unwrap()
}

/// 切换主题外观：跟随系统走系统外观同步，亮 / 暗直接切换；随后底色极性自动对齐
pub(crate) fn set_theme_choice(choice: ThemeChoice, window: Option<&mut Window>, cx: &mut App) {
    *THEME_CHOICE.lock().unwrap() = choice;
    crate::prefs::save_theme_choice(choice);
    apply_theme(choice, window, cx);
}

/// 应用主题外观并让底色极性对齐（亮色主题配亮色底色组）
fn apply_theme(choice: ThemeChoice, window: Option<&mut Window>, cx: &mut App) {
    match choice {
        ThemeChoice::System => Theme::sync_system_appearance(window, cx),
        ThemeChoice::Light => Theme::change(ThemeMode::Light, None, cx),
        ThemeChoice::Dark => Theme::change(ThemeMode::Dark, None, cx),
    }
    reconcile_skin(cx);
}

/// 启动装配：恢复持久化的设置并应用主题（未持久化过默认跟随系统）
pub(crate) fn init_theme(cx: &mut App) {
    if let Some(choice) = crate::prefs::load_theme_choice() {
        *THEME_CHOICE.lock().unwrap() = choice;
    }
    if let Some(skin) = crate::prefs::load_skin() {
        *PANEL_SKIN.lock().unwrap() = skin;
    }
    apply_theme(theme_choice(), None, cx);
}

pub struct PanelView {
    focus: FocusHandle,
    view: View,
    slider_state: Entity<SliderState>,
    /// 系统外观变化监听（Subscription 被 drop 即失效，必须随视图持有）
    _appearance_sub: Subscription,
    /// 窗口激活变化监听：失焦即收起
    _activation_sub: Subscription,
}

impl PanelView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 聚焦根视图，键盘事件（Esc）才能路由进来
        let focus = cx.focus_handle();
        focus.focus(window, cx);

        // 面板窗口可能在上次外观同步之后才创建：选了"跟随系统"先对齐当前系统外观
        if theme_choice() == ThemeChoice::System {
            Theme::sync_system_appearance(Some(window), cx);
            reconcile_skin(cx);
        }
        // 系统外观变化时实时跟随（仅"跟随系统"模式生效；固定亮 / 暗时忽略）
        let _appearance_sub = window.observe_window_appearance(|window, cx| {
            if theme_choice() == ThemeChoice::System {
                Theme::sync_system_appearance(Some(window), cx);
                reconcile_skin(cx);
            }
        });

        let _activation_sub =
            cx.observe_window_activation(window, |_, window, _| app::dismiss_on_deactivate(window));
        // 每 2s 通知重绘；窗口关闭（update 失败）时退出循环
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs_f64(sampler::TICK_SECS))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        // 透明度滑动条状态（设置视图使用；初始值取当前皮肤，拖动即时生效并持久化）
        let slider_state = cx.new(|_| {
            SliderState::new()
                .min(30.0)
                .max(100.0)
                .step(5.0)
                .default_value(panel_skin().alpha * 100.0)
        });
        cx.subscribe(&slider_state, move |_, _, event, cx| {
            if let SliderEvent::Change(SliderValue::Single(x))
                | SliderEvent::Release(SliderValue::Single(x)) = event
            {
                set_panel_alpha(x / 100.0);
                cx.notify();
            }
        })
        .detach();
        Self {
            focus,
            view: View::Home,
            slider_state,
            _appearance_sub,
            _activation_sub,
        }
    }

    /// 切换视图：同步采样的明细资源与窗口高度
    pub(crate) fn apply_view(&mut self, view: View) {
        self.view = view;
        crate::sampler::set_active(match view {
            View::Detail(r) => Some(r.kind()),
            _ => None,
        });
    }

    fn view_h(&self) -> f32 {
        app::view_h(&self.view)
    }


}

impl Render for PanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let skin = panel_skin();
        // 透明度同步到窗口毛玻璃层（模糊层 alphaValue，见 app::sync_glass_alpha）
        crate::app::sync_glass_alpha(window, skin.alpha);
        let packed = ((skin.base[0] * 255.0) as u32) << 16
            | ((skin.base[1] * 255.0) as u32) << 8
            | ((skin.base[2] * 255.0) as u32);
        let mut panel_bg: Hsla = rgb(packed).into();
        panel_bg.a = skin.alpha;

        let root = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if this.view == View::Home {
                        window.remove_window();
                    } else {
                        // 子视图先返回主页
                        this.apply_view(View::Home);
                        app::set_panel_frame(window, app::VIEW_HOME_H);
                        cx.notify();
                    }
                }
            }))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .w(px(app::PANEL_W))
            .h(px(self.view_h()))
            .bg(panel_bg)
            .rounded(px(14.))
            .overflow_hidden()
            .text_color(theme.foreground)
            .text_size(px(13.));

        match self.view {
            View::Home => root.child(home::render(cx, &theme, &sampler::snapshot())),
            View::Settings => root.child(settings::render(cx, &theme, &self.slider_state)),
            View::Detail(r) => root.child(detail::render(cx, &theme, r, &sampler::snapshot())),
        }
    }
}

// ---------- 共享部件 ----------

/// 卡片外壳：圆角 + 描边 + 主题底色
/// 分区标题
/// 键值行：左标签右数值
/// 图例行：色点 + 标签 + 右对齐数值
pub(crate) fn legend_row(
    theme: &Theme,
    color: Hsla,
    label: &'static str,
    value: String,
) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .child(
            div()
                .flex()
                .gap_1()
                .items_center()
                .child(div().w(px(7.)).h(px(7.)).rounded(px(3.5)).bg(color))
                .child(div().text_color(theme.muted_foreground).child(label)),
        )
        .child(
            div()
                .text_color(theme.foreground)
                .child(SharedString::from(value)),
        )
}

/// 网络行：彩色图标 + 标签 + 右对齐数值
pub(crate) fn net_row(
    theme: &Theme,
    icon: IconName,
    color: Hsla,
    label: &'static str,
    value: String,
) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .child(
            div()
                .flex()
                .gap_1()
                .items_center()
                .child(Icon::new(icon).size(px(13.)).text_color(color))
                .child(div().text_color(theme.muted_foreground).child(label)),
        )
        .child(
            div()
                .text_color(theme.foreground)
                .child(SharedString::from(value)),
        )
}

/// 趋势图：浅色填充 + 原色描边（与 CPU 堆叠色带同风格），统一悬停格式（几秒前 / 一位小数）
pub(crate) fn trend_chart(
    data: Vec<(usize, f32)>,
    color: gpui_kit::Hsla,
    label: &'static str,
    height: f32,
) -> impl IntoElement {
    div().h(px(height)).child(
        AreaChart::<(usize, f32), String, f32>::new(data)
            .x(|p| p.0.to_string())
            .y(|p| p.1)
            .fill(lighten(color, 0.6))
            .stroke(color)
            .name(label)
            .tooltip_title(|p| SharedString::from(format!("{}s {}", p.0, tr("前", "ago"))))
            .tooltip_value(|_, _, v| SharedString::from(format!("{:.1}%", v)))
            .y_domain(0.0, 100.0)
            .x_axis(false)
            .grid(false)
            .linear(),
    )
}

/// 大数字（主题前景色，亮 / 暗主题下都保持可读）
pub(crate) fn big_value(theme: &Theme, value: String) -> impl IntoElement {
    div()
        .text_size(px(26.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .child(SharedString::from(value))
}

/// 由历史数据构造趋势图数据点（x = 几秒前，供悬停提示显示时间语义）
pub(crate) fn chart_series(
    hist: &Hist,
) -> (
    Vec<(usize, f32)>,
    Vec<(usize, f32)>,
    Vec<(usize, f32)>,
) {
    (
        series_points(&hist.cpu_user),
        series_points(&hist.cpu_sys),
        series_points(&hist.mem),
    )
}

/// 磁盘读 / 写趋势数据点
pub(crate) fn disk_series(hist: &Hist) -> (Vec<(usize, f32)>, Vec<(usize, f32)>) {
    (series_points(&hist.disk_read), series_points(&hist.disk_write))
}

/// 历史队列 → 等距数据点（x = 几秒前）
fn series_points(q: &std::collections::VecDeque<f32>) -> Vec<(usize, f32)> {
    let ago = |i: usize, len: usize| ((len - 1 - i) as f64 * sampler::TICK_SECS) as usize;
    q.iter()
        .enumerate()
        .map(|(i, v)| (ago(i, q.len()), *v))
        .collect()
}

/// CPU 双折线悬停数据点：几秒前 + 用户态/系统态占比
#[derive(Clone, Copy)]
struct CpuPoint {
    ago: usize,
    user: f32,
    sys: f32,
}

/// 数据色向白色靠拢（保持不透明）：面积填充用浅色、描边保持原色。
/// 注意不能用半透明——堆叠依赖后画的色带完全盖住下层，半透明会在重叠区混色。
fn lighten(c: Hsla, amount: f32) -> Hsla {
    Hsla {
        l: c.l + (1.0 - c.l) * amount,
        ..c
    }
}

/// CPU 趋势图：系统活动监视器式堆叠色带——系统态（红）在底部、用户态（蓝）叠于其上，
/// 两条带永远不重叠。AreaChart 多序列按添加顺序绘制（各自从 0 叠画）：
/// 先画「用户+系统」总占用带（蓝），再画「系统」带（红）覆盖其底部 0..sys 区间，
/// 即得到红底蓝顶的堆叠；色带为浅色填充、描边保持原色；悬停按序列分两行显示。
pub(crate) fn cpu_trend_chart(
    user: Vec<(usize, f32)>,
    sys: Vec<(usize, f32)>,
    height: f32,
) -> impl IntoElement {
    let points: Vec<CpuPoint> = user
        .iter()
        .zip(sys.iter())
        .map(|((ago, user), (_, sys))| CpuPoint {
            ago: *ago,
            user: *user,
            sys: *sys,
        })
        .collect();
    // 品牌数据色：用户态蓝 / 系统态红（描边原色，填充向白变浅 60%）
    let user_color: Hsla = rgb(0x0a84ff).into();
    let sys_color: Hsla = rgb(0xff453a).into();
    let user_fill = lighten(user_color, 0.6);
    let sys_fill = lighten(sys_color, 0.6);
    div().h(px(height)).child(
        AreaChart::<CpuPoint, String, f32>::new(points)
            .x(|p| p.ago.to_string())
            // 第一层：总占用（用户 + 系统），浅蓝填充
            .y(|p| p.user + p.sys)
            .fill(user_fill)
            .stroke(user_color)
            .name(tr("用户", "User"))
            .linear()
            // 第二层：系统占用，浅红填充覆盖底部，露出上方蓝色用户带
            .y(|p| p.sys)
            .fill(sys_fill)
            .stroke(sys_color)
            .name(tr("系统", "System"))
            .linear()
            .tooltip_title(|p| SharedString::from(format!("{}s {}", p.ago, tr("前", "ago"))))
            .tooltip_value(|p, ix, _| {
                SharedString::from(if ix == 0 {
                    format!("{} {:.1}%", tr("用户", "User"), p.user)
                } else {
                    format!("{} {:.1}%", tr("系统", "System"), p.sys)
                })
            })
            .y_domain(0.0, 100.0)
            .x_axis(false)
            .grid(false),
    )
}

/// 磁盘 IO 蝶形图（参考系统活动监视器）：读取带自中线向上、写入带自中线向下，
/// 纵向按窗口内峰值速率自动缩放（速率无固定上限，与活动监视器口径一致）。
/// 图表组件的面积图基线固定在图底、div 无垂直翻转，画不出中线镜像——用 canvas 手绘，
/// 浅填充 + 深描边，与 CPU 堆叠色带同风格。
pub(crate) fn disk_io_chart(
    read: Vec<(usize, f32)>,
    write: Vec<(usize, f32)>,
    height: f32,
) -> impl IntoElement {
    let reads: Vec<f32> = read.into_iter().map(|(_, v)| v).collect();
    let writes: Vec<f32> = write.into_iter().map(|(_, v)| v).collect();
    let read_color: Hsla = rgb(0xbf5af2).into();
    let write_color: Hsla = rgb(0xff375f).into();
    let read_fill = lighten(read_color, 0.6);
    let write_fill = lighten(write_color, 0.6);
    div().h(px(height)).child(
        // canvas 默认布局尺寸为 0，必须显式撑满容器
        canvas(|_, _, _| (), move |bounds, (), window, _| {
            let n = reads.len().min(writes.len());
            if n < 2 {
                return;
            }
            let w = bounds.size.width.as_f32();
            let h = bounds.size.height.as_f32();
            let left = bounds.origin.x.as_f32();
            let mid = bounds.origin.y.as_f32() + h / 2.0;
            let half = h / 2.0;
            let peak = reads
                .iter()
                .chain(writes.iter())
                .copied()
                .fold(0.0f32, f32::max)
                .max(1.0);
            let x_at = |i: usize| left + w * i as f32 / (n - 1) as f32;
            let draw_band =
                |values: &[f32], up: bool, fill: Hsla, stroke: Hsla, window: &mut Window| {
                    let pts: Vec<Point<Pixels>> = values[..n]
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            let y = if up {
                                mid - v / peak * half
                            } else {
                                mid + v / peak * half
                            };
                            point(px(x_at(i)), px(y))
                        })
                        .collect();
                    // 填充：中线两端与曲线围合
                    let mut fill_builder = PathBuilder::fill();
                    fill_builder.move_to(point(px(x_at(0)), px(mid)));
                    for p in &pts {
                        fill_builder.line_to(*p);
                    }
                    fill_builder.line_to(point(px(x_at(n - 1)), px(mid)));
                    if let Ok(path) = fill_builder.build() {
                        window.paint_path(path, fill);
                    }
                    // 描边：曲线本身
                    let mut stroke_builder = PathBuilder::stroke(px(1.5));
                    for (i, p) in pts.iter().enumerate() {
                        if i == 0 {
                            stroke_builder.move_to(*p);
                        } else {
                            stroke_builder.line_to(*p);
                        }
                    }
                    if let Ok(path) = stroke_builder.build() {
                        window.paint_path(path, stroke);
                    }
                };
            draw_band(&reads, true, read_fill, read_color, window);
            draw_band(&writes, false, write_fill, write_color, window);
        })
        .w_full()
        .h_full(),
    )
}

/// 令牌色按面板透明度调整（卡片与根底色保持同一透明度联动）
pub(crate) fn panel_color(c: Hsla) -> Hsla {
    let mut c = c;
    c.a *= panel_skin().alpha;
    c
}

/// 卡片底色：主题 group_box 令牌 × 面板透明度
pub(crate) fn card_bg(theme: &Theme) -> Hsla {
    panel_color(theme.group_box)
}
