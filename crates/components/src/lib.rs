//! FluxDown 应用自有的 gpui-base 视觉封装。
//!
//! gpui-base 提供交互、键盘与无障碍语义；本 crate 只负责从完整主题 token
//! 组装稳定的 shadcn 风格。业务组件依赖这里，不直接散落颜色和尺寸字面量。

use fluxdown_ui_theme::active_theme;
use gpui::{
    App, Div, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, div, px, relative,
};
use gpui_base::Button;

/// 基础按钮的视觉语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Destructive,
}

/// 创建具备键盘、焦点、无障碍与完整交互态的主题按钮。
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    cx: &App,
) -> Button {
    let tokens = active_theme(cx).tokens();
    let label = label.into();
    let palette = ButtonPalette::for_variant(variant, tokens.colors);

    Button::new(id)
        .h(tokens.spacing.xxl + tokens.spacing.xs)
        .px(tokens.spacing.md)
        .line_height(relative(1.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .rounded(tokens.radius.md)
        .border_1()
        .border_color(palette.border)
        .bg(palette.background)
        .text_color(palette.foreground)
        .text_size(tokens.typography.sm.size)
        .font_weight(tokens.typography.sm.weight)
        .hover(move |style| style.bg(palette.hover))
        .active(move |style| style.bg(palette.active))
        .focus_visible(move |style| style.border_color(tokens.colors.ring))
        .styles(|styles| styles.disabled(|style| style.opacity(0.5)))
        .accessibility_label(label.clone())
        .child(label)
}
/// 创建带前置图标的主要操作按钮。
pub fn primary_icon_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: impl IntoElement,
    cx: &App,
) -> Button {
    let tokens = active_theme(cx).tokens();
    let label = label.into();
    let palette = ButtonPalette::for_variant(ButtonVariant::Primary, tokens.colors);

    Button::new(id)
        .h(px(32.))
        .px(tokens.spacing.md)
        .line_height(relative(1.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .rounded(tokens.radius.md)
        .border_1()
        .border_color(palette.border)
        .bg(palette.background)
        .text_color(palette.foreground)
        .text_size(tokens.typography.sm.size)
        .font_weight(tokens.typography.sm.weight)
        .hover(move |style| style.bg(palette.hover))
        .active(move |style| style.bg(palette.active))
        .focus_visible(move |style| style.border_color(tokens.colors.ring))
        .accessibility_label(label.clone())
        .child(
            div()
                .flex()
                .items_center()
                .gap(tokens.spacing.sm)
                .child(icon)
                .child(label),
        )
}

/// 创建任务列表工具栏的紧凑图标操作按钮。
pub fn toolbar_action_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: impl IntoElement,
    destructive: bool,
    cx: &App,
) -> Button {
    let tokens = active_theme(cx).tokens();
    let foreground = if destructive {
        tokens.colors.destructive
    } else {
        tokens.colors.muted_foreground
    };

    Button::new(id)
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .rounded(tokens.radius.md)
        .bg(transparent(tokens.colors.muted))
        .text_color(foreground)
        .hover(move |style| style.bg(tokens.colors.muted))
        .active(move |style| style.bg(tokens.colors.secondary))
        .focus_visible(move |style| style.bg(tokens.colors.muted))
        .accessibility_label(label)
        .child(icon)
}

/// 创建侧栏导航按钮；选中态由调用方控制。
pub fn navigation_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    let tokens = active_theme(cx).tokens();
    button(id, label, ButtonVariant::Ghost, cx)
        .w_full()
        .justify_start()
        .selected(selected)
        .styles(|styles| {
            styles.selected(|style| {
                style
                    .bg(tokens.colors.accent)
                    .text_color(tokens.colors.accent_foreground)
            })
        })
}
/// 创建带图标与尾部信息的侧栏导航按钮。
pub fn sidebar_navigation_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: impl IntoElement,
    trailing: impl IntoElement,
    selected: bool,
    cx: &App,
) -> Button {
    let tokens = active_theme(cx).tokens();
    let label = label.into();
    let hover_background = if selected {
        tokens.colors.accent
    } else {
        tokens.colors.muted
    };

    Button::new(id)
        .h(px(32.))
        .w_full()
        .px(tokens.spacing.sm)
        .line_height(relative(1.))
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .rounded(tokens.radius.md)
        .bg(transparent(tokens.colors.muted))
        .text_color(tokens.colors.muted_foreground)
        .text_size(px(12.5))
        .font_weight(FontWeight::NORMAL)
        .hover(move |style| style.bg(hover_background))
        .active(move |style| style.bg(hover_background))
        .focus_visible(move |style| style.bg(hover_background))
        .selected(selected)
        .styles(|styles| {
            styles.selected(|style| {
                style
                    .bg(tokens.colors.accent)
                    .text_color(tokens.colors.accent_foreground)
                    .font_weight(FontWeight::MEDIUM)
            })
        })
        .accessibility_label(label.clone())
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(tokens.spacing.sm)
                .child(icon)
                .child(div().truncate().child(label)),
        )
        .child(trailing)
}

/// 创建活动栏按钮。
///
/// 选中态使用淡蓝色强调背景；未选中项仅在悬浮时显示中性灰背景。
pub fn activity_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: impl IntoElement,
    selected: bool,
    cx: &App,
) -> Button {
    let tokens = active_theme(cx).tokens();
    let colors = tokens.colors;
    let selected_foreground = colors.accent_foreground;
    let hover_background = if selected {
        colors.accent
    } else {
        colors.muted
    };

    Button::new(id)
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .rounded(tokens.radius.lg)
        .bg(transparent(colors.muted))
        .text_color(colors.muted_foreground)
        .hover(move |style| style.bg(hover_background))
        .active(move |style| style.bg(hover_background))
        .focus_visible(move |style| style.bg(hover_background))
        .selected(selected)
        .styles(|styles| {
            styles.selected(|style| style.bg(colors.accent).text_color(selected_foreground))
        })
        .accessibility_label(label)
        .child(icon)
}

/// 创建使用 surface、border、radius 与 shadow token 的基础卡片。
pub fn card(cx: &App) -> Div {
    let tokens = active_theme(cx).tokens();
    div()
        .bg(tokens.colors.surface)
        .text_color(tokens.colors.surface_foreground)
        .border_1()
        .border_color(tokens.colors.border)
        .rounded(tokens.radius.lg)
        .shadow(tokens.shadow.sm.clone())
}

#[derive(Clone, Copy)]
struct ButtonPalette {
    background: Hsla,
    foreground: Hsla,
    border: Hsla,
    hover: Hsla,
    active: Hsla,
}

impl ButtonPalette {
    fn for_variant(variant: ButtonVariant, colors: fluxdown_ui_theme::ColorTokens) -> Self {
        match variant {
            ButtonVariant::Primary => Self::filled(colors.primary, colors.primary_foreground),
            ButtonVariant::Secondary => Self {
                background: colors.secondary,
                foreground: colors.secondary_foreground,
                border: colors.border,
                hover: colors.accent,
                active: shift_lightness(colors.accent, 0.08),
            },
            ButtonVariant::Ghost => Self {
                background: transparent(colors.background),
                foreground: colors.foreground,
                border: transparent(colors.border),
                hover: colors.accent,
                active: shift_lightness(colors.accent, 0.08),
            },
            ButtonVariant::Destructive => {
                Self::filled(colors.destructive, colors.destructive_foreground)
            }
        }
    }

    fn filled(background: Hsla, foreground: Hsla) -> Self {
        Self {
            background,
            foreground,
            border: background,
            hover: shift_toward_contrast(background, 0.08),
            active: shift_toward_contrast(background, 0.13),
        }
    }
}

fn transparent(color: Hsla) -> Hsla {
    Hsla { a: 0., ..color }
}

fn shift_toward_contrast(color: Hsla, amount: f32) -> Hsla {
    let delta = if color.l >= 0.5 { -amount } else { amount };
    shift_lightness(color, delta)
}

fn shift_lightness(color: Hsla, delta: f32) -> Hsla {
    Hsla {
        l: (color.l + delta).clamp(0., 1.),
        ..color
    }
}
