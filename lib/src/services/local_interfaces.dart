import 'dart:io';

/// 本机一张网卡上的一个 IPv4 地址。
///
/// [name] 是操作系统给出的接口名，**各平台形态完全不同**且不做归一化（归一化
/// 必然靠猜，反而丢信息）：Windows 是本地化友好名（`WLAN` / `以太网 2` /
/// `vEthernet (WSL)`），Linux 是内核名（`wlan0` / `eth0` / `docker0`），
/// macOS 是 BSD 名（`en0` / `en1` / `bridge100` / `utun3`）。UI 只把它当作
/// 给用户辨认用的后缀展示。
class LocalInterface {
  final String name;
  final String ip;

  const LocalInterface({required this.name, required this.ip});

  @override
  bool operator ==(Object other) =>
      other is LocalInterface && other.name == name && other.ip == ip;

  @override
  int get hashCode => Object.hash(name, ip);

  @override
  String toString() => '$ip · $name';
}

/// 本机非回环 IPv4 网卡的枚举与排序。
///
/// 用途：局域网 / 组网访问开启后，向用户展示「对端应该连哪个地址」。这里的
/// 排序只影响**展示与默认选中项**，服务实际监听恒为 `0.0.0.0`。
///
/// 跨平台前提：
/// - 枚举走 Dart SDK 的 [NetworkInterface.list]（POSIX `getifaddrs` /
///   Windows `GetAdaptersAddresses`），Windows / Linux / macOS（Intel 与
///   Apple Silicon 同一份 Dart 代码，无架构相关分支）/ Android / iOS 全部支持。
///   （`NetworkInterface.listSupported` 在现行 SDK 里已废弃并恒为 true，所以
///   不做能力探测，只靠 catch 兜住运行期失败。）
/// - macOS 未开 App Sandbox（`macos/Runner/*.entitlements` 里
///   `com.apple.security.app-sandbox` = false），`getifaddrs` 不需要额外授权；
///   即便将来开启沙箱，该调用也不属受限 API，最坏情况被 catch 兜为空表。
class LocalInterfaces {
  const LocalInterfaces._();

  /// 回环地址：关闭局域网访问时唯一可用的展示地址。
  static const String loopback = '127.0.0.1';

  /// 枚举本机非回环、非链路本地（169.254/16）IPv4，并按 [sortForLan] 排序。
  /// 任何平台限制/权限失败都降级为空表——调用方据此只展示回环地址。
  static Future<List<LocalInterface>> list() async {
    try {
      final ifaces = await NetworkInterface.list(
        includeLoopback: false,
        includeLinkLocal: false,
        type: InternetAddressType.IPv4,
      );
      return sortForLan([
        for (final iface in ifaces)
          for (final addr in iface.addresses)
            LocalInterface(name: iface.name, ip: addr.address),
      ]);
    } catch (_) {
      return const [];
    }
  }

  /// 按「对端最可能连得上」排序，纯函数、不丢项。
  ///
  /// 系统枚举顺序不可依赖：三个平台都可能把虚拟适配器（Hyper-V / WSL /
  /// Docker / libvirt / VMware / Tailscale / ZeroTier / VPN 隧道）排在真实网
  /// 卡前面，而默认选中项取的就是首项。规则：先按是否疑似虚拟网卡分档（见
  /// [isLikelyVirtual]），再按私网段优先级 `192.168/16 → 10/8 → 172.16/12 →
  /// 其余`。虚拟网卡只降权不剔除——自建组网（Tailscale / ZeroTier）场景里它
  /// 才是唯一可达地址。
  ///
  /// 同档保持系统原序：`List.sort` 不稳定，这里用原下标兜底。
  static List<LocalInterface> sortForLan(List<LocalInterface> ifaces) {
    final indexed = [
      for (var i = 0; i < ifaces.length; i++) (i: i, e: ifaces[i]),
    ]..sort((a, b) {
      final byRank = rank(a.e).compareTo(rank(b.e));
      return byRank != 0 ? byRank : a.i.compareTo(b.i);
    });
    return [for (final it in indexed) it.e];
  }

  /// 排序权重，越小越优先。虚拟网卡整体退到物理网卡之后。
  static int rank(LocalInterface iface) =>
      (isLikelyVirtual(iface.name) ? 10 : 0) + _subnetRank(iface.ip);

  /// 私网段优先级：家庭/办公局域网最常见的 192.168/16 最优，其次 10/8、
  /// 172.16/12，其余（公网地址、CGNAT 100.64/10 等）最后。
  static int _subnetRank(String ip) {
    if (ip.startsWith('192.168.')) return 0;
    if (ip.startsWith('10.')) return 1;
    final parts = ip.split('.');
    if (parts.length == 4 && parts[0] == '172') {
      final second = int.tryParse(parts[1]);
      if (second != null && second >= 16 && second <= 31) return 2;
    }
    return 3;
  }

  /// 按接口名判断是否疑似虚拟 / 隧道网卡（三平台常见命名的并集，大小写无关）。
  ///
  /// 只用于降权，判错的代价仅是排序位置，不影响可选项完整性——因此宁可宽松：
  /// Windows `vEthernet (…)` / `VMware Network Adapter …` / `VirtualBox
  /// Host-Only …` / `Tailscale` / `ZeroTier One …`；Linux `docker0` /
  /// `br-xxxx` / `veth…` / `virbr0` / `tun0` / `tap0` / `wg0` / `zt…` /
  /// `tailscale0`；macOS `bridge100` / `utun…` / `vmnet…` / `awdl0` / `llw0`。
  static bool isLikelyVirtual(String name) {
    final n = name.toLowerCase();
    for (final needle in _virtualNameHints) {
      if (n.contains(needle)) return true;
    }
    // `br-<id>` 是 Docker 自建桥；裸 `br0` 也按桥处理。veth/utun/tun/tap 同族
    // 前缀单独判，避免 `contains` 把 `Ethernet` 这类真名误伤。
    return n.startsWith('br-') ||
        n.startsWith('br0') ||
        n.startsWith('veth') ||
        n.startsWith('utun') ||
        n.startsWith('tun') ||
        n.startsWith('tap') ||
        n.startsWith('wg') ||
        n.startsWith('zt');
  }

  static const List<String> _virtualNameHints = [
    'vethernet',
    'virbr',
    'docker',
    'vmware',
    'vmnet',
    'virtualbox',
    'hyper-v',
    'tailscale',
    'zerotier',
    'wireguard',
    'openvpn',
    'bridge1',
    'awdl',
    'llw',
    'loopback',
    'pseudo-interface',
  ];
}
