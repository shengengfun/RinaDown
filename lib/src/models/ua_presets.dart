/// 预设 UA 映射（key → UA 字符串），全端共享的唯一事实源。
///
/// Chrome / Edge 遵循 UA Reduction 策略，次版本号固定为 0.0.0；
/// Edge 额外携带完整的小版本号（Edg/145.0.3800.70）以匹配官方实际发送的格式。
/// 版本基准：Chrome 145 / Edge 145 / Firefox 147 / Safari 18.3（2025-2026 主流版本）
const kUaPresets = <String, String>{
  // Chrome 145（UA Reduction：Win11 与 Win10 发送同一 UA，次版本号全为 0）
  'chrome':
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 '
      '(KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36',
  // Firefox 147（Gecko/20100101 为固定占位，仅主版本号暴露）
  'firefox':
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:147.0) '
      'Gecko/20100101 Firefox/147.0',
  // Edge 145（基于 Chromium，追加 Edg/ 标记；注意是 Edg 而非 Edge）
  'edge':
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 '
      '(KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.3800.70',
  // Safari 18.3（macOS Sonoma；WebKit 版本号 605.1.15 长期固定）
  'safari':
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) '
      'AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Safari/605.1.15',
};

/// 根据 UA 字符串反推预设 key。
///
/// 空字符串 → 'default'（全局设置里 = 引擎内置 FluxDown 标识；
/// 任务/队列级 = 继承上层设置），命中预设 → 对应 key，否则 → 'custom'。
String detectUaPreset(String ua) {
  if (ua.isEmpty) return 'default';
  for (final entry in kUaPresets.entries) {
    if (entry.value == ua) return entry.key;
  }
  return 'custom';
}
