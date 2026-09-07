// 下载/上传限速输入框的单位选择（设置页「下载限速」上方）与换算。
//
// 可用单位：kbps / KB/s / MB/s / mbps（1024 进制、与旧 KB/s=*1024 一致）：
//   - kbps = 千比特/秒  → *1024/8 = *128 B/s
//   - KB/s = 千字节/秒  → *1024 B/s
//   - mbps = 兆比特/秒  → *1024*1024/8 = *131072 B/s
//   - MB/s = 兆字节/秒  → *1024*1024 B/s

/// 可选的限速单位（显示值与持久化值一致；默认 = 旧行为 KB/s）。
const List<String> kSpeedLimitUnits = ['KB/s', 'MB/s', 'kbps', 'mbps'];

const String kDefaultSpeedLimitUnit = 'KB/s';

/// 单位 → 每「1」对应的字节/秒（1024 进制）。
int speedLimitUnitDivisor(String unit) => switch (unit) {
  'kbps' => 128,
  'KB/s' => 1024,
  'mbps' => 131072,
  _ => 1048576, // 'MB/s'（及未知回退）
};

/// 用户输入数值 → 字节/秒（向下取整）。
int bytesFromValue(String unit, int value) =>
    value * speedLimitUnitDivisor(unit);

/// 字节/秒 → 该单位下的显示数值（整除）。
int valueFromBytes(String unit, int bytes) =>
    bytes ~/ speedLimitUnitDivisor(unit);
