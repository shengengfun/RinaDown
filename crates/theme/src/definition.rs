use gpui::{Hsla, Rgba, SharedString, rgb};
use gpui_base::{
    ColorTokens, RadiusTokens, SemanticThemeTokens, ShadowTokens, SpacingTokens, TypographyTokens,
};
use gpui_component::ThemeMode;
use serde::{Deserialize, Serialize};

use crate::{AccentScheme, BuiltinThemeId};

/// FluxDown GPUI 主题文件的当前 schema 版本。
pub const THEME_SCHEMA_VERSION: u32 = 1;
const SHADOW_ALPHA: f32 = 46.0 / 255.0;
/// Flutter `accentBackground` alpha 的 8-bit 量化：0.10 / 0.15 / 0.18。
const ACCENT_ALPHA_LIGHT: u8 = 26;
const ACCENT_ALPHA_SOFT: u8 = 38;
const ACCENT_ALPHA_DARK: u8 = 46;

/// 一套同时覆盖亮色与暗色的完整 Base 语义 token。
///
/// 字段保持公开，主题编辑器可直接配置全部 17 个颜色、6 个圆角、7 个间距、
/// 两个字体族、6 个字号/行高/字重角色和 3 档阴影。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FluxThemeDefinition {
    pub schema_version: u32,
    pub name: SharedString,
    pub author: Option<SharedString>,
    pub light: SemanticThemeTokens,
    pub dark: SemanticThemeTokens,
}

impl FluxThemeDefinition {
    /// Flutter 客户端 `defaultLight` / `defaultDark` 使用的 FluxDown 默认主题。
    pub fn fluxdown_default() -> Self {
        Self::builtin_pair(BuiltinThemeId::DefaultDark, BuiltinThemeId::DefaultLight)
    }

    /// 单个内置预设：预设自身外观的槽位取其调色板，另一槽位沿用默认主题。
    pub fn builtin(id: BuiltinThemeId) -> Self {
        match id.appearance() {
            ThemeMode::Dark => Self::builtin_pair(id, BuiltinThemeId::DefaultLight),
            ThemeMode::Light => Self::builtin_pair(BuiltinThemeId::DefaultDark, id),
        }
    }

    /// 暗色槽位与亮色槽位各取一个内置预设（Flutter `selectedDarkTheme` / `selectedLightTheme`）。
    pub fn builtin_pair(dark: BuiltinThemeId, light: BuiltinThemeId) -> Self {
        let dark_palette = palette(dark);
        let light_palette = palette(light);
        let name = if dark == BuiltinThemeId::DefaultDark && light == BuiltinThemeId::DefaultLight {
            SharedString::from("FluxDown Default")
        } else {
            SharedString::from(format!("{} / {}", dark_palette.name, light_palette.name))
        };
        Self {
            schema_version: THEME_SCHEMA_VERSION,
            name,
            author: Some("FluxDown".into()),
            light: semantic_tokens(color_tokens(light_palette)),
            dark: semantic_tokens(color_tokens(dark_palette)),
        }
    }

    /// 套用强调色方案：与 Flutter 预设工厂的 `accent` 参数一致，重算亮暗两套
    /// token 中由强调色派生的 `primary` / `accent` / `ring` 系列。
    #[must_use]
    pub fn with_accent(mut self, scheme: AccentScheme, custom_argb: u32) -> Self {
        let accent = scheme.color(custom_argb);
        apply_accent(&mut self.light.colors, accent);
        apply_accent(&mut self.dark.colors, accent);
        self
    }

    /// 返回指定明暗模式的完整 token 快照。
    pub fn tokens(&self, mode: ThemeMode) -> &SemanticThemeTokens {
        match mode {
            ThemeMode::Light => &self.light,
            ThemeMode::Dark => &self.dark,
        }
    }

    /// 返回指定明暗模式的完整可变 token 快照。
    pub fn tokens_mut(&mut self, mode: ThemeMode) -> &mut SemanticThemeTokens {
        match mode {
            ThemeMode::Light => &mut self.light,
            ThemeMode::Dark => &mut self.dark,
        }
    }
}

impl Default for FluxThemeDefinition {
    fn default() -> Self {
        Self::fluxdown_default()
    }
}

fn semantic_tokens(colors: ColorTokens) -> SemanticThemeTokens {
    let typography = TypographyTokens {
        sans: "MiSans".into(),
        ..TypographyTokens::default()
    };

    SemanticThemeTokens {
        colors,
        radius: RadiusTokens::default(),
        spacing: SpacingTokens::default(),
        typography,
        shadow: ShadowTokens::elevations(Hsla {
            h: 0.,
            s: 0.,
            l: 0.,
            a: SHADOW_ALPHA,
        }),
    }
}

/// Flutter `FluxThemeTokens` 工厂中映射到 Base token 的 Layer0 颜色。
struct Palette {
    name: &'static str,
    accent: u32,
    accent_background_alpha: u8,
    /// Flutter 固定的 `accentForeground`（如 Nord）；`None` 按强调色亮度自动取黑/白。
    accent_foreground: Option<u32>,
    background: u32,
    surface1: u32,
    surface2: u32,
    text_primary: u32,
    text_secondary: u32,
    border: u32,
    status_error: u32,
}

fn palette(id: BuiltinThemeId) -> &'static Palette {
    match id {
        BuiltinThemeId::DefaultDark => &Palette {
            name: "Default Dark",
            accent: 0x3B82F6,
            accent_background_alpha: ACCENT_ALPHA_DARK,
            accent_foreground: None,
            background: 0x1C1C1E,
            surface1: 0x2C2C2E,
            surface2: 0x3A3A3C,
            text_primary: 0xF5F5F7,
            text_secondary: 0xA1A1A6,
            border: 0x48484A,
            status_error: 0xEF4444,
        },
        BuiltinThemeId::DefaultLight => &Palette {
            name: "Default Light",
            accent: 0x3B82F6,
            accent_background_alpha: ACCENT_ALPHA_LIGHT,
            accent_foreground: None,
            background: 0xF8F9FA,
            surface1: 0xFFFFFF,
            surface2: 0xF1F3F5,
            text_primary: 0x09090B,
            text_secondary: 0x71717A,
            border: 0xE4E4E7,
            status_error: 0xEF4444,
        },
        BuiltinThemeId::MidnightBlue => &Palette {
            name: "Midnight Blue",
            accent: 0x60A5FA,
            accent_background_alpha: ACCENT_ALPHA_SOFT,
            accent_foreground: None,
            background: 0x0F172A,
            surface1: 0x1E293B,
            surface2: 0x334155,
            text_primary: 0xF1F5F9,
            text_secondary: 0x94A3B8,
            border: 0x334155,
            status_error: 0xEF4444,
        },
        BuiltinThemeId::Nord => &Palette {
            name: "Nord",
            accent: 0x88C0D0,
            accent_background_alpha: ACCENT_ALPHA_SOFT,
            accent_foreground: Some(0x2E3440),
            background: 0x2E3440,
            surface1: 0x3B4252,
            surface2: 0x434C5E,
            text_primary: 0xECEFF4,
            text_secondary: 0xD8DEE9,
            border: 0x4C566A,
            status_error: 0xBF616A,
        },
        BuiltinThemeId::WarmLight => &Palette {
            name: "Warm Light",
            accent: 0xE11D48,
            accent_background_alpha: ACCENT_ALPHA_LIGHT,
            accent_foreground: None,
            background: 0xFFFBEB,
            surface1: 0xFFFFFF,
            surface2: 0xFEF3C7,
            text_primary: 0x1C1917,
            text_secondary: 0x78716C,
            border: 0xE7E5E4,
            status_error: 0xDC2626,
        },
    }
}

impl BuiltinThemeId {
    /// 预设自带强调色的颜色 token（主题卡片预览用，不受用户强调色影响）。
    #[must_use]
    pub fn colors(self) -> ColorTokens {
        color_tokens(palette(self))
    }
}

/// Flutter Layer0 → Base 语义 token 的固定映射。
fn color_tokens(palette: &Palette) -> ColorTokens {
    let accent = color(palette.accent);
    ColorTokens {
        background: color(palette.background),
        foreground: color(palette.text_primary),
        surface: color(palette.surface1),
        surface_foreground: color(palette.text_primary),
        primary: accent,
        primary_foreground: palette
            .accent_foreground
            .map_or_else(|| foreground_for(accent), color),
        secondary: color(palette.surface2),
        secondary_foreground: color(palette.text_primary),
        muted: color(palette.surface2),
        muted_foreground: color(palette.text_secondary),
        accent: accent.alpha(f32::from(palette.accent_background_alpha) / 255.),
        accent_foreground: accent,
        destructive: color(palette.status_error),
        destructive_foreground: color(0xFFFFFF),
        border: color(palette.border),
        input: color(palette.border),
        ring: accent,
    }
}

/// 重算由强调色派生的 token。`primary_foreground` 仅在原值是按亮度自动
/// 推导时才跟随新强调色；预设固定的前景（Nord）保持不变，与 Flutter 一致。
fn apply_accent(colors: &mut ColorTokens, accent: Hsla) {
    let auto_foreground = colors.primary_foreground == foreground_for(colors.primary);
    colors.primary = accent;
    if auto_foreground {
        colors.primary_foreground = foreground_for(accent);
    }
    colors.accent = accent.alpha(colors.accent.a);
    colors.accent_foreground = accent;
    colors.ring = accent;
}

/// Flutter `_foregroundFor`：强调色相对亮度 > 0.5 取近黑，否则取白。
#[must_use]
pub fn foreground_for(accent: Hsla) -> Hsla {
    if relative_luminance(accent) > 0.5 {
        color(0x09090B)
    } else {
        color(0xFFFFFF)
    }
}

/// WCAG 相对亮度（Flutter `Color.computeLuminance`）。
fn relative_luminance(value: Hsla) -> f32 {
    fn linear(channel: f32) -> f32 {
        if channel <= 0.039_28 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }
    let rgba = Rgba::from(value);
    0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b)
}

fn color(value: u32) -> Hsla {
    Hsla::from(rgb(value))
}

#[cfg(test)]
mod tests {
    use gpui::{Hsla, px, rgb};
    use gpui_base::ColorTokens;
    use gpui_component::ThemeMode;

    use super::FluxThemeDefinition;
    use crate::{AccentScheme, BuiltinThemeId};

    fn color(value: u32) -> Hsla {
        Hsla::from(rgb(value))
    }

    fn color_with_alpha(value: u32, alpha: f32) -> Hsla {
        Hsla {
            a: alpha,
            ..color(value)
        }
    }

    #[test]
    fn default_light_colors_match_flutter_tokens() {
        let theme = FluxThemeDefinition::fluxdown_default();

        assert_eq!(
            theme.light.colors,
            ColorTokens {
                background: color(0xF8F9FA),
                foreground: color(0x09090B),
                surface: color(0xFFFFFF),
                surface_foreground: color(0x09090B),
                primary: color(0x3B82F6),
                primary_foreground: color(0xFFFFFF),
                secondary: color(0xF1F3F5),
                secondary_foreground: color(0x09090B),
                muted: color(0xF1F3F5),
                muted_foreground: color(0x71717A),
                accent: color_with_alpha(0x3B82F6, 26.0 / 255.0),
                accent_foreground: color(0x3B82F6),
                destructive: color(0xEF4444),
                destructive_foreground: color(0xFFFFFF),
                border: color(0xE4E4E7),
                input: color(0xE4E4E7),
                ring: color(0x3B82F6),
            }
        );
        assert_eq!(theme.light.radius.md, px(6.));
        assert_eq!(theme.light.spacing.xxl, px(32.));
        assert_eq!(theme.light.typography.md.size, px(16.));
        assert!(!theme.light.shadow.md.is_empty());
    }

    #[test]
    fn default_dark_colors_match_flutter_tokens() {
        let theme = FluxThemeDefinition::fluxdown_default();

        assert_eq!(
            theme.dark.colors,
            ColorTokens {
                background: color(0x1C1C1E),
                foreground: color(0xF5F5F7),
                surface: color(0x2C2C2E),
                surface_foreground: color(0xF5F5F7),
                primary: color(0x3B82F6),
                primary_foreground: color(0xFFFFFF),
                secondary: color(0x3A3A3C),
                secondary_foreground: color(0xF5F5F7),
                muted: color(0x3A3A3C),
                muted_foreground: color(0xA1A1A6),
                accent: color_with_alpha(0x3B82F6, 46.0 / 255.0),
                accent_foreground: color(0x3B82F6),
                destructive: color(0xEF4444),
                destructive_foreground: color(0xFFFFFF),
                border: color(0x48484A),
                input: color(0x48484A),
                ring: color(0x3B82F6),
            }
        );
    }

    #[test]
    fn full_token_snapshot_round_trips_for_theme_editing() -> Result<(), serde_json::Error> {
        let mut theme = FluxThemeDefinition::fluxdown_default();
        theme.tokens_mut(ThemeMode::Light).spacing.xxl = px(40.);

        let json = serde_json::to_string(&theme)?;
        let restored = serde_json::from_str::<FluxThemeDefinition>(&json)?;

        assert_eq!(restored, theme);
        Ok(())
    }

    #[test]
    fn builtin_presets_map_flutter_layer0_colors() {
        let nord = FluxThemeDefinition::builtin(BuiltinThemeId::Nord);
        assert_eq!(nord.dark.colors.background, color(0x2E3440));
        assert_eq!(nord.dark.colors.surface, color(0x3B4252));
        assert_eq!(nord.dark.colors.muted, color(0x434C5E));
        assert_eq!(nord.dark.colors.muted_foreground, color(0xD8DEE9));
        assert_eq!(nord.dark.colors.primary, color(0x88C0D0));
        assert_eq!(nord.dark.colors.primary_foreground, color(0x2E3440));
        assert_eq!(
            nord.dark.colors.accent,
            color_with_alpha(0x88C0D0, 38.0 / 255.0)
        );
        assert_eq!(nord.dark.colors.destructive, color(0xBF616A));
        assert_eq!(nord.dark.colors.border, color(0x4C566A));
        assert_eq!(nord.light, FluxThemeDefinition::fluxdown_default().light);

        let warm = FluxThemeDefinition::builtin(BuiltinThemeId::WarmLight);
        assert_eq!(warm.light.colors.background, color(0xFFFBEB));
        assert_eq!(warm.light.colors.primary, color(0xE11D48));
        assert_eq!(warm.light.colors.primary_foreground, color(0xFFFFFF));
        assert_eq!(
            warm.light.colors.accent,
            color_with_alpha(0xE11D48, 26.0 / 255.0)
        );
        assert_eq!(warm.light.colors.destructive, color(0xDC2626));
        assert_eq!(warm.dark, FluxThemeDefinition::fluxdown_default().dark);

        let midnight = FluxThemeDefinition::builtin(BuiltinThemeId::MidnightBlue);
        assert_eq!(midnight.dark.colors.background, color(0x0F172A));
        assert_eq!(midnight.dark.colors.ring, color(0x60A5FA));

        let pair =
            FluxThemeDefinition::builtin_pair(BuiltinThemeId::Nord, BuiltinThemeId::WarmLight);
        assert_eq!(pair.dark, nord.dark);
        assert_eq!(pair.light, warm.light);
        assert_eq!(pair.name.as_ref(), "Nord / Warm Light");
    }

    #[test]
    fn accent_rewrites_derived_tokens_and_keeps_alpha() {
        let theme = FluxThemeDefinition::fluxdown_default().with_accent(AccentScheme::Rose, 0);
        let rose = color(0xF43F5E);
        for tokens in [&theme.light, &theme.dark] {
            assert_eq!(tokens.colors.primary, rose);
            assert_eq!(tokens.colors.primary_foreground, color(0xFFFFFF));
            assert_eq!(tokens.colors.accent_foreground, rose);
            assert_eq!(tokens.colors.ring, rose);
        }
        assert_eq!(
            theme.light.colors.accent,
            color_with_alpha(0xF43F5E, 26.0 / 255.0)
        );
        assert_eq!(
            theme.dark.colors.accent,
            color_with_alpha(0xF43F5E, 46.0 / 255.0)
        );
        assert_eq!(theme.light.colors.background, color(0xF8F9FA));

        let custom =
            FluxThemeDefinition::fluxdown_default().with_accent(AccentScheme::Custom, 0xFFFA_FAFA);
        assert_eq!(custom.light.colors.primary, color(0xFAFAFA));
        assert_eq!(custom.light.colors.primary_foreground, color(0x09090B));

        let unchanged =
            FluxThemeDefinition::fluxdown_default().with_accent(AccentScheme::Blue, 0xFF00_0000);
        assert_eq!(unchanged, FluxThemeDefinition::fluxdown_default());
    }

    #[test]
    fn accent_preserves_pinned_preset_foreground() {
        let nord =
            FluxThemeDefinition::builtin(BuiltinThemeId::Nord).with_accent(AccentScheme::Green, 0);
        assert_eq!(nord.dark.colors.primary, color(0x22C55E));
        assert_eq!(nord.dark.colors.primary_foreground, color(0x2E3440));
        assert_eq!(nord.light.colors.primary_foreground, color(0xFFFFFF));
    }
}
