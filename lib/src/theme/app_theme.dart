import 'package:flutter/material.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import 'flux_theme_tokens.dart';

/// MiSans 字体族名（与 pubspec.yaml 中声明的 family 一致）
const _fontFamily = 'MiSans';

/// 构建紧凑的按钮尺寸主题
ShadButtonSizesTheme _buttonSizes(FluxThemeTokens tokens) {
  final m = tokens.metric;
  return ShadButtonSizesTheme(
    regular: ShadButtonSizeTheme(
      height: m.buttonHeightMd,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
    ),
    sm: ShadButtonSizeTheme(
      height: m.buttonHeightSm,
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 2),
    ),
    lg: ShadButtonSizeTheme(
      height: m.buttonHeightLg,
      padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 6),
    ),
    icon: ShadButtonSizeTheme(
      height: m.buttonHeightMd,
      width: m.buttonHeightMd,
      padding: EdgeInsets.zero,
    ),
  );
}

/// input / select 统一字段内边距。默认 12/8 偏高（字段 ≈36px，比按钮
/// 32px 高一头）；收到 10/6 后字段 ≈32px，与 buttonHeightMd 对齐。
const _fieldPadding = EdgeInsets.symmetric(horizontal: 10, vertical: 6);

/// input / select 的统一字段装饰。
///
/// 关键：把此前从未接入 ShadTheme 的 `inputBackground` token 落到字段
/// 填充上——浅色主题下字段与面板同为纯白、仅剩一圈细边框的「贴纸感」
/// 即源于此缺口。边框走 `inputBorder`/`radiusInput` token；焦点环仍由
/// shadcn 全局 decoration 主题提供，此处不覆盖。
ShadDecoration _fieldDecoration(FluxThemeTokens tokens) {
  return ShadDecoration(
    color: tokens.inputBackground,
    border: ShadBorder.all(
      radius: BorderRadius.circular(tokens.metric.radiusInput),
      color: tokens.inputBorder,
      width: 1,
    ),
  );
}

/// select 下拉主题：字段装饰/内边距与 input 一致；`effects: []` 让选项
/// 列表点击即现——默认 150ms 渐显+缩放在高频操作里只显得迟钝。
ShadSelectTheme _selectTheme(FluxThemeTokens tokens) {
  return ShadSelectTheme(
    padding: _fieldPadding,
    effects: const [],
    decoration: _fieldDecoration(tokens),
  );
}

/// input 主题：光标色 + 紧凑内边距 + 字段装饰。
///
/// 字号 13：默认 muted 14px 是字段虚高的另一半来源（14×1.4+12+2 ≈ 37px）；
/// 13px 后整字段 ≈32px，与按钮同高。仅覆盖 fontSize，字族/字色仍随主题。
ShadInputTheme _inputTheme(FluxThemeTokens tokens) {
  const fieldTextStyle = TextStyle(fontSize: 13);
  return ShadInputTheme(
    cursorColor: tokens.accent,
    padding: _fieldPadding,
    style: fieldTextStyle,
    placeholderStyle: fieldTextStyle,
    decoration: _fieldDecoration(tokens),
  );
}

// ═══════════════════════════════════════════════════════════
//  从 FluxThemeTokens 构建 ShadThemeData
// ═══════════════════════════════════════════════════════════

/// 缓存
FluxThemeTokens? _cachedTokens;
ShadThemeData? _cachedThemeData;
/// 对话框进出场动效：120ms 淡入 + 轻微缩放（默认 300ms 偏慢，观感拖沓）。
const _dialogAnimateIn = <AnimateEffect<dynamic>>[
  FadeEffect(duration: Duration(milliseconds: 120)),
  ScaleEffect(
    begin: Offset(.97, .97),
    end: Offset(1, 1),
    duration: Duration(milliseconds: 120),
    curve: Curves.easeOutCubic,
  ),
];
const _dialogAnimateOut = <AnimateEffect<dynamic>>[
  FadeEffect(begin: 1, end: 0, duration: Duration(milliseconds: 90)),
];

/// 从 [FluxThemeTokens] 构建 [ShadThemeData]。
///
/// 结果会被缓存，相同 tokens 不会重复构建。
ShadThemeData buildThemeFromTokens(FluxThemeTokens tokens) {
  if (identical(_cachedTokens, tokens) || _cachedTokens == tokens) {
    return _cachedThemeData!;
  }
  _cachedTokens = tokens;

  final isDark = tokens.appearance == Brightness.dark;
  final colorScheme = _buildColorScheme(tokens, isDark);

  if (isDark) {
    _cachedThemeData = ShadThemeData(
      brightness: Brightness.dark,
      colorScheme: colorScheme,
      radius: BorderRadius.circular(tokens.metric.radiusMd),
      textTheme: ShadTextTheme(family: _fontFamily),
      buttonSizesTheme: _buttonSizes(tokens),
      ghostButtonTheme: ShadButtonTheme(
        hoverBackgroundColor: tokens.elementHover,
      ),
      outlineButtonTheme: ShadButtonTheme(
        hoverBackgroundColor: tokens.elementHover,
      ),
      switchTheme: ShadSwitchTheme(
        thumbColor: tokens.switchThumb,
        uncheckedTrackColor: tokens.switchTrack,
      ),
      inputTheme: _inputTheme(tokens),
      selectTheme: _selectTheme(tokens),
      primaryDialogTheme: ShadDialogTheme(
        animateIn: _dialogAnimateIn,
        animateOut: _dialogAnimateOut,
        backgroundColor: tokens.dialogBackground,
        border: Border.all(color: tokens.border, width: 1),
        shadows: [
          BoxShadow(
            color: tokens.shadow.withValues(alpha: tokens.metric.alphaShadowStrong),
            blurRadius: 24,
            offset: const Offset(0, 8),
          ),
        ],
      ),
      alertDialogTheme: ShadDialogTheme(
        animateIn: _dialogAnimateIn,
        animateOut: _dialogAnimateOut,
        backgroundColor: tokens.dialogBackground,
        border: Border.all(color: tokens.border, width: 1),
        shadows: [
          BoxShadow(
            color: tokens.shadow.withValues(alpha: tokens.metric.alphaShadowStrong),
            blurRadius: 24,
            offset: const Offset(0, 8),
          ),
        ],
      ),
      primaryToastTheme: _primaryToastTheme(tokens),
      destructiveToastTheme: _destructiveToastTheme(tokens, colorScheme),
    );
  } else {
    _cachedThemeData = ShadThemeData(
      brightness: Brightness.light,
      colorScheme: colorScheme,
      radius: BorderRadius.circular(tokens.metric.radiusMd),
      textTheme: ShadTextTheme(family: _fontFamily),
      buttonSizesTheme: _buttonSizes(tokens),
      ghostButtonTheme: ShadButtonTheme(
        hoverBackgroundColor: tokens.elementHover,
      ),
      outlineButtonTheme: ShadButtonTheme(
        hoverBackgroundColor: tokens.elementHover,
      ),
      inputTheme: _inputTheme(tokens),
      selectTheme: _selectTheme(tokens),
      primaryDialogTheme: const ShadDialogTheme(
        animateIn: _dialogAnimateIn,
        animateOut: _dialogAnimateOut,
      ),
      alertDialogTheme: const ShadDialogTheme(
        animateIn: _dialogAnimateIn,
        animateOut: _dialogAnimateOut,
      ),
      primaryToastTheme: _primaryToastTheme(tokens),
      destructiveToastTheme: _destructiveToastTheme(tokens, colorScheme),
    );
  }

  return _cachedThemeData!;
}

/// Toast 通用布局：内容自适应宽度、仅限最大宽。
/// 不设 minWidth——ShadToast 内部 Row 会铺满约束，短文案会变成大片空白的白板
/// （视觉上像"边距被涂了背景色"）；过宽还会被 ShadSonner 的 ClipRect 裁掉右侧圆角。
/// 尾部 34 = 关闭按钮预留（8 边距 + 20 图标 + 6 间隙），否则悬浮 × 会压住文字。
const _toastPadding = EdgeInsetsDirectional.fromSTEB(16, 12, 34, 12);
const _toastConstraints = BoxConstraints(maxWidth: 380);

List<BoxShadow> _toastShadows(FluxThemeTokens tokens) => [
  BoxShadow(
    color: tokens.shadow.withValues(alpha: tokens.metric.alphaShadowSoft),
    blurRadius: 16,
    offset: const Offset(0, 6),
  ),
  BoxShadow(
    color: tokens.shadow.withValues(alpha: tokens.metric.alphaShadowFaint),
    blurRadius: 4,
    offset: const Offset(0, 2),
  ),
];

/// 普通 toast — 悬浮卡片底色 + 紧凑排版
ShadToastTheme _primaryToastTheme(FluxThemeTokens tokens) {
  return ShadToastTheme(
    backgroundColor: tokens.dialogBackground,
    border: ShadBorder.all(color: tokens.border, width: 1),
    radius: BorderRadius.circular(tokens.metric.radiusDialog),
    shadows: _toastShadows(tokens),
    padding: _toastPadding,
    constraints: _toastConstraints,
    // Row 收拢到内容宽度，卡片随文案长度自适应。
    // 关闭按钮定位带拉伸满高（配合 FluxSonner 注入的 Center 包裹按钮），任意高度垂直居中。
    closeIconPosition: const ShadPosition(top: 0, bottom: 0, right: 8),
    mainAxisSize: MainAxisSize.min,
    titleStyle: TextStyle(
      fontFamily: _fontFamily,
      fontSize: 13,
      fontWeight: FontWeight.w500,
      color: tokens.textPrimary,
    ),
    descriptionStyle: TextStyle(
      fontFamily: _fontFamily,
      fontSize: 12,
      color: tokens.textSecondary,
    ),
  );
}

/// 错误 toast — 保留 destructive 底色，排版与普通 toast 一致
ShadToastTheme _destructiveToastTheme(
  FluxThemeTokens tokens,
  ShadColorScheme colorScheme,
) {
  return ShadToastTheme(
    backgroundColor: colorScheme.destructive,
    border: ShadBorder.all(
      color: colorScheme.destructive.withValues(alpha: 0.5),
      width: 1,
    ),
    radius: BorderRadius.circular(tokens.metric.radiusDialog),
    shadows: _toastShadows(tokens),
    padding: _toastPadding,
    constraints: _toastConstraints,
    mainAxisSize: MainAxisSize.min,
    closeIconPosition: const ShadPosition(top: 0, bottom: 0, right: 8),
    titleStyle: TextStyle(
      fontFamily: _fontFamily,
      fontSize: 13,
      fontWeight: FontWeight.w500,
      color: colorScheme.destructiveForeground,
    ),
    descriptionStyle: TextStyle(
      fontFamily: _fontFamily,
      fontSize: 12,
      color: colorScheme.destructiveForeground.withValues(alpha: 0.9),
    ),
  );
}

/// 从 Token 构建 ShadColorScheme
ShadColorScheme _buildColorScheme(FluxThemeTokens tokens, bool isDark) {
  // 获取基底 scheme（用于保留 shadcn 的 destructive 等未覆盖色）
  final base = isDark
      ? _baseDarkScheme(tokens.accent)
      : _baseLightScheme(tokens.accent);

  return base.copyWith(
    background: tokens.background,
    foreground: tokens.textPrimary,
    primary: tokens.accent,
    primaryForeground: tokens.accentForeground,
    card: tokens.surface1,
    cardForeground: tokens.textPrimary,
    popover: tokens.surface1,
    popoverForeground: tokens.textPrimary,
    secondary: tokens.surface2,
    secondaryForeground: tokens.textPrimary,
    muted: tokens.surface2,
    mutedForeground: tokens.textSecondary,
    accent: tokens.surface2,
    accentForeground: tokens.textPrimary,
    border: tokens.border,
    input: tokens.border,
    ring: tokens.accent,
    selection: tokens.elementSelected,
  );
}

/// 获取基底 dark scheme（保留 destructive 等）
ShadColorScheme _baseDarkScheme(Color accent) {
  // 使用 Zinc 基底（中性灰，最适合覆盖）
  return const ShadZincColorScheme.dark();
}

/// 获取基底 light scheme
ShadColorScheme _baseLightScheme(Color accent) {
  return const ShadZincColorScheme.light();
}
