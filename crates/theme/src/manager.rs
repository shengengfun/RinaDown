use std::collections::BTreeMap;
use std::sync::Arc;

use gpui::{App, Global, Window, linear_color_stop, linear_gradient};
use gpui_component::{Theme as ComponentTheme, ThemeMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AppearancePreferences, FluxThemeDefinition, SemanticThemeTokens, normalize_ui_scale_percent,
};

const TABLE_HOVER_TOP_OPACITY: f32 = 0.78;
const TABLE_HOVER_BOTTOM_OPACITY: f32 = 0.48;

/// 用户主题偏好；`System` 在每次安装时解析当前系统明暗模式。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    fn resolve(self, cx: &App) -> ThemeMode {
        match self {
            Self::System => cx.window_appearance().into(),
            Self::Light => ThemeMode::Light,
            Self::Dark => ThemeMode::Dark,
        }
    }
}

/// 当前应用主题的完整快照。
///
/// gpui-component 的 legacy `Theme` 无法持久保存自定义 spacing/shadow；本全局状态
/// 因此保留完整 Base token，并在每次切换后重新投影到 `gpui_base::Theme`。
#[derive(Clone)]
pub struct FluxThemeState {
    definition: Arc<FluxThemeDefinition>,
    appearance: AppearancePreferences,
    mode: ThemeMode,
    tokens: SemanticThemeTokens,
}

impl Global for FluxThemeState {}

impl FluxThemeState {
    /// 当前主题定义（未缩放）。
    pub fn definition(&self) -> &Arc<FluxThemeDefinition> {
        &self.definition
    }

    /// 用户当前的外观选项（内置主题、强调色、缩放、明暗偏好）。
    pub fn appearance(&self) -> &AppearancePreferences {
        &self.appearance
    }

    /// 用户选择的明暗偏好。
    pub fn preference(&self) -> ThemePreference {
        self.appearance.theme_mode
    }

    /// 已解析的实际明暗模式。
    pub fn mode(&self) -> ThemeMode {
        self.mode
    }

    /// 界面缩放百分比（80 ~ 150）。
    pub fn ui_scale_percent(&self) -> u16 {
        self.appearance.ui_scale_percent
    }

    /// 当前完整 Base token（已按界面缩放）；应用自有组件应只从这里取值。
    pub fn tokens(&self) -> &SemanticThemeTokens {
        &self.tokens
    }
}

/// 在 `gpui_component::init` 后安装与 Flutter 客户端一致的默认主题。
pub fn init(cx: &mut App) {
    let appearance = AppearancePreferences::default();
    install(Arc::new(appearance.definition()), appearance, None, cx);
}

/// 返回当前完整主题状态。
///
/// 调用方必须先执行 [`init`] 或 [`install_theme`]；桌面 shell 在创建任何 view 前
/// 建立这一不变量。
pub fn active_theme(cx: &App) -> &FluxThemeState {
    cx.global::<FluxThemeState>()
}

/// 安装任意主题定义并把完整 token 同步到 gpui-component 与 gpui-base。
///
/// 其余外观选项（强调色、缩放）沿用当前状态；未初始化时取默认值。
pub fn install_theme(
    definition: Arc<FluxThemeDefinition>,
    preference: ThemePreference,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let mut appearance = cx
        .try_global::<FluxThemeState>()
        .map_or_else(AppearancePreferences::default, |state| state.appearance);
    appearance.theme_mode = preference;
    install(definition, appearance, window, cx);
}

/// 应用一组外观选项。内置主题/强调色未变时沿用当前定义（含通过
/// [`install_theme`] 装入的自定义定义），否则按内置预设重新解析。
pub fn set_appearance(
    appearance: AppearancePreferences,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let definition = match cx.try_global::<FluxThemeState>() {
        Some(state) if state.appearance.same_palette(&appearance) => Arc::clone(&state.definition),
        _ => Arc::new(appearance.definition()),
    };
    install(definition, appearance, window, cx);
}

/// 偏好快照 → 外观。读取 `appearance.theme_mode` / `appearance.dark_theme` /
/// `appearance.light_theme` / `appearance.color_scheme` / `appearance.custom_color` /
/// `ui_scale`；与当前状态一致时不做任何事，可在每次快照/偏好事件上幂等调用。
pub fn apply_appearance_preferences(values: &BTreeMap<String, Value>, cx: &mut App) {
    let appearance = AppearancePreferences::from_values(values);
    if cx
        .try_global::<FluxThemeState>()
        .is_some_and(|state| state.appearance == appearance)
    {
        return;
    }
    set_appearance(appearance, None, cx);
}

/// 保留当前主题定义，仅切换明暗偏好。
pub fn set_theme_preference(
    preference: ThemePreference,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let mut appearance = active_theme(cx).appearance;
    appearance.theme_mode = preference;
    set_appearance(appearance, window, cx);
}

/// 设置界面缩放百分比（限制到 80 ~ 150，按 10 取整）。
///
/// GPUI 只有逐窗口的 rem 尺寸；gpui-component 的 `Root` 每帧把
/// `Theme::font_size` 写入窗口 rem，因此这里通过缩放排版/间距/圆角 token
/// 驱动所有窗口，无需逐窗口调用 `set_rem_size`。
pub fn set_ui_scale(percent: u16, cx: &mut App) {
    let mut appearance = active_theme(cx).appearance;
    appearance.ui_scale_percent = normalize_ui_scale_percent(percent);
    set_appearance(appearance, None, cx);
}

/// 在亮/暗两种显式模式间切换。
pub fn toggle_theme(window: &mut Window, cx: &mut App) {
    let preference = if active_theme(cx).mode().is_dark() {
        ThemePreference::Light
    } else {
        ThemePreference::Dark
    };
    set_theme_preference(preference, Some(window), cx);
}

/// 系统外观变化时刷新 `System` 偏好；显式亮/暗偏好保持不变。
pub fn sync_system_theme(window: &mut Window, cx: &mut App) {
    if active_theme(cx).preference() == ThemePreference::System {
        set_theme_preference(ThemePreference::System, Some(window), cx);
    }
}

fn install(
    definition: Arc<FluxThemeDefinition>,
    appearance: AppearancePreferences,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let mode = appearance.theme_mode.resolve(cx);
    let mut tokens = definition.tokens(mode).clone();
    scale_tokens(&mut tokens, appearance.ui_scale());

    ComponentTheme::change(mode, None, cx);
    {
        let component_theme = ComponentTheme::global_mut(cx);
        component_theme.apply_semantic_tokens(&tokens);
        component_theme.focus_ring = false;
        component_theme.title_bar = tokens.colors.surface;
        component_theme.title_bar_border = tokens.colors.border;
        // gpui-component 的 Sidebar / Settings 侧栏只读 sidebar_* 系列，legacy
        // `apply_semantic_tokens` 不会同步它们；不映射就会留在库默认的黑/白。
        component_theme.sidebar = tokens.colors.surface;
        component_theme.sidebar_foreground = tokens.colors.surface_foreground;
        component_theme.sidebar_border = tokens.colors.border;
        component_theme.sidebar_accent = tokens.colors.accent;
        component_theme.sidebar_accent_foreground = tokens.colors.accent_foreground;
        component_theme.sidebar_primary = tokens.colors.primary;
        component_theme.sidebar_primary_foreground = tokens.colors.primary_foreground;
        component_theme.tokens.sidebar = tokens.colors.surface.into();
        component_theme.tokens.sidebar_foreground = tokens.colors.surface_foreground.into();
        component_theme.tokens.sidebar_border = tokens.colors.border.into();
        component_theme.tokens.sidebar_accent = tokens.colors.accent.into();
        component_theme.tokens.sidebar_accent_foreground = tokens.colors.accent_foreground.into();
        component_theme.tokens.sidebar_primary = tokens.colors.primary.into();
        component_theme.tokens.sidebar_primary_foreground = tokens.colors.primary_foreground.into();
        // legacy Button/Link 系列同样不在 apply_semantic_tokens 内；不同步则 gpui-component
        // 的 `Button::primary()` 永远是库默认黑/白，不跟随强调色。
        let colors = tokens.colors;
        let primary_hover = shift_toward_contrast(colors.primary, 0.08);
        let primary_active = shift_toward_contrast(colors.primary, 0.13);
        let secondary_hover = shift_toward_contrast(colors.secondary, 0.05);
        let secondary_active = shift_toward_contrast(colors.secondary, 0.09);
        let danger_hover = shift_toward_contrast(colors.destructive, 0.08);
        let danger_active = shift_toward_contrast(colors.destructive, 0.13);
        component_theme.primary_hover = primary_hover;
        component_theme.primary_active = primary_active;
        component_theme.secondary_hover = secondary_hover;
        component_theme.secondary_active = secondary_active;
        component_theme.danger_hover = danger_hover;
        component_theme.danger_active = danger_active;
        component_theme.link = colors.primary;
        component_theme.button_primary = colors.primary;
        component_theme.button_primary_foreground = colors.primary_foreground;
        component_theme.button_primary_hover = primary_hover;
        component_theme.button_primary_active = primary_active;
        component_theme.button_secondary = colors.secondary;
        component_theme.button_secondary_foreground = colors.secondary_foreground;
        component_theme.button_secondary_hover = secondary_hover;
        component_theme.button_secondary_active = secondary_active;
        component_theme.button_danger = colors.destructive;
        component_theme.button_danger_foreground = colors.destructive_foreground;
        component_theme.button_danger_hover = danger_hover;
        component_theme.button_danger_active = danger_active;
        component_theme.tokens.primary_hover = primary_hover.into();
        component_theme.tokens.primary_active = primary_active.into();
        component_theme.tokens.secondary_hover = secondary_hover.into();
        component_theme.tokens.secondary_active = secondary_active.into();
        component_theme.tokens.danger_hover = danger_hover.into();
        component_theme.tokens.danger_active = danger_active.into();
        component_theme.tokens.link = colors.primary.into();
        component_theme.tokens.button_primary = colors.primary.into();
        component_theme.tokens.button_primary_foreground = colors.primary_foreground.into();
        component_theme.tokens.button_primary_hover = primary_hover.into();
        component_theme.tokens.button_primary_active = primary_active.into();
        component_theme.tokens.button_secondary = colors.secondary.into();
        component_theme.tokens.button_secondary_foreground = colors.secondary_foreground.into();
        component_theme.tokens.button_secondary_hover = secondary_hover.into();
        component_theme.tokens.button_secondary_active = secondary_active.into();
        component_theme.tokens.button_danger = colors.destructive.into();
        component_theme.tokens.button_danger_foreground = colors.destructive_foreground.into();
        component_theme.tokens.button_danger_hover = danger_hover.into();
        component_theme.tokens.button_danger_active = danger_active.into();
        component_theme.table = tokens.colors.surface;
        component_theme.table_active = tokens.colors.accent;
        component_theme.table_active_border = tokens.colors.primary;
        component_theme.table_even = tokens.colors.surface;
        component_theme.table_head = tokens.colors.muted;
        component_theme.table_head_foreground = tokens.colors.muted_foreground;
        component_theme.table_hover = tokens.colors.muted;
        component_theme.table_row_border = tokens.colors.border;
        component_theme.tokens.table = tokens.colors.surface.into();
        component_theme.tokens.table_active = tokens.colors.accent.into();
        component_theme.tokens.table_even = tokens.colors.surface.into();
        component_theme.tokens.table_head = tokens.colors.muted.into();
        component_theme.tokens.table_hover.background = linear_gradient(
            180.,
            linear_color_stop(tokens.colors.muted.opacity(TABLE_HOVER_TOP_OPACITY), 0.),
            linear_color_stop(tokens.colors.muted.opacity(TABLE_HOVER_BOTTOM_OPACITY), 1.),
        );
    }
    ComponentTheme::sync_base(cx);
    gpui_base::Theme::global_mut(cx).tokens = tokens.clone();
    cx.set_global(FluxThemeState {
        definition,
        appearance,
        mode,
        tokens,
    });

    for handle in cx.windows() {
        let _ = handle.update(cx, |_, window, _| window.refresh());
    }
    if let Some(window) = window {
        window.refresh();
    }
}

/// 按界面缩放倍率放大尺寸类 token；颜色与字重不变。
fn scale_tokens(tokens: &mut SemanticThemeTokens, scale: f32) {
    if scale == 1. {
        return;
    }
    let radius = &mut tokens.radius;
    radius.sm *= scale;
    radius.md *= scale;
    radius.lg *= scale;
    radius.xl *= scale;

    let spacing = &mut tokens.spacing;
    spacing.xxs *= scale;
    spacing.xs *= scale;
    spacing.sm *= scale;
    spacing.md *= scale;
    spacing.lg *= scale;
    spacing.xl *= scale;
    spacing.xxl *= scale;

    let typography = &mut tokens.typography;
    for text in [
        &mut typography.xs,
        &mut typography.sm,
        &mut typography.md,
        &mut typography.lg,
        &mut typography.xl,
        &mut typography.mono_md,
    ] {
        text.size *= scale;
        text.line_height *= scale;
    }

    let shadow = &mut tokens.shadow;
    for level in [&mut shadow.sm, &mut shadow.md, &mut shadow.lg] {
        for box_shadow in level.iter_mut() {
            box_shadow.offset.x *= scale;
            box_shadow.offset.y *= scale;
            box_shadow.blur_radius *= scale;
            box_shadow.spread_radius *= scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::scale_tokens;
    use crate::FluxThemeDefinition;

    #[test]
    fn scale_tokens_scales_sizes_and_keeps_colors() {
        let definition = FluxThemeDefinition::fluxdown_default();
        let mut tokens = definition.light.clone();
        scale_tokens(&mut tokens, 1.5);

        assert_eq!(tokens.typography.md.size, px(24.));
        assert_eq!(tokens.typography.sm.line_height, px(30.));
        assert_eq!(tokens.spacing.md, px(18.));
        assert_eq!(tokens.radius.md, px(9.));
        assert_eq!(tokens.radius.full, definition.light.radius.full);
        assert_eq!(tokens.shadow.md[0].blur_radius, px(12.));
        assert_eq!(tokens.colors, definition.light.colors);

        let mut unchanged = definition.light.clone();
        scale_tokens(&mut unchanged, 1.);
        assert_eq!(unchanged, definition.light);
    }
}

/// 向对比方向偏移亮度：亮色变暗、暗色变亮（hover / active 派生）。
fn shift_toward_contrast(color: gpui::Hsla, amount: f32) -> gpui::Hsla {
    let delta = if color.l >= 0.5 { -amount } else { amount };
    gpui::Hsla {
        l: (color.l + delta).clamp(0., 1.),
        ..color
    }
}
