// 预设 UA（与桌面端 lib/src/models/ua_presets.dart 保持同步）。
// Chrome / Edge 遵循 UA Reduction 策略，次版本号固定为 0.0.0；
// 版本基准：Chrome 145 / Edge 145 / Firefox 147 / Safari 18.3（2025-2026 主流版本）
export const UA_PRESETS = [
  {
    label: 'Chrome',
    value:
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36',
  },
  {
    label: 'Firefox',
    value: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:147.0) Gecko/20100101 Firefox/147.0',
  },
  {
    label: 'Edge',
    value:
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.3800.70',
  },
  {
    label: 'Safari',
    value:
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Safari/605.1.15',
  },
]
