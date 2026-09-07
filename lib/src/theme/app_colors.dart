import 'package:flutter/widgets.dart';

import 'flux_theme_tokens.dart';

/// 主题感知色板 — 通过 AppColors.of(context) 获取
///
/// 所有颜色从 [FluxThemeTokens] 读取，API 与旧版完全兼容。
/// 当用户自定义主题时，每个颜色值都可独立覆盖。
class AppColors {
  final FluxThemeTokens _tokens;

  const AppColors._(this._tokens);

  factory AppColors.of(BuildContext context) {
    return AppColors._(FluxThemeScope.of(context));
  }

  /// 直接从 token 构造（供不依赖 context 的场景使用）
  factory AppColors.fromTokens(FluxThemeTokens tokens) = AppColors._;

  /// 原始 tokens 访问（供高级场景使用）
  FluxThemeTokens get tokens => _tokens;

  // ── Backgrounds ──
  Color get bg => _tokens.background;
  Color get surface1 => _tokens.surface1;
  Color get surface2 => _tokens.surface2;
  Color get surface3 => _tokens.surface3;

  // ── Hover ──
  Color get hoverBg => _tokens.elementHover;

  // ── Borders ──
  Color get border => _tokens.border;

  // ── Text ──
  Color get textPrimary => _tokens.textPrimary;
  Color get textSecondary => _tokens.textSecondary;
  Color get textMuted => _tokens.textMuted;
  Color get textDisabled => _tokens.textDisabled;

  // ── Accent ──
  Color get accent => _tokens.accent;
  Color get accentHover => _tokens.accentHover;
  Color get accentBg => _tokens.accentBackground;
  Color get accentForeground => _tokens.accentForeground;

  // ── 选中行 ──
  Color get selectedBg => _tokens.elementSelected;

  // ── Input ──
  Color get inputFocusBg => _tokens.inputFocusBackground;
  Color get inputBg => _tokens.inputBackground;
  Color get inputBorder => _tokens.inputBorder;
  Color get inputFocusBorder => _tokens.inputFocusBorder;

  // ── Dialog ──
  Color get dialogBarrier => _tokens.dialogBarrier;
  Color get dialogBg => _tokens.dialogBackground;

  // ── Switch ──
  Color get switchTrack => _tokens.switchTrack;
  Color get switchThumb => _tokens.switchThumb;

  // ── Shadow ──
  Color get shadow => _tokens.shadow;

  // ── Status（实例方法 — 跟随主题 token，用于新代码）──
  Color get statusSuccess => _tokens.statusSuccess;
  Color get statusWarning => _tokens.statusWarning;
  Color get statusError => _tokens.statusError;

  // ── Status（静态常量 — 向后兼容旧代码的 AppColors.green 用法）──
  static const green = Color(0xFF22C55E);
  static const amber = Color(0xFFF59E0B);
  static const red = Color(0xFFEF4444);

  // ── File category（静态常量 — 文件类型着色）──
  // 色值沿用 manifest 原型规格（manifest.js `MF_EXT_TYPE` + styles.css `.mf-ftile.t-*`）：
  // 只有 video / audio 需要专属色，image→green、archive→amber、document→accent、
  // program/other→中性，均复用既有 token，不新增主题字段（不动 schemaVersion）。
  static const categoryVideo = Color(0xFFA855F7);
  static const categoryAudio = Color(0xFF06B6D4);

  // ── Segment Palette ──
  List<Color> get segmentPalette => _tokens.segmentPalette;
}
