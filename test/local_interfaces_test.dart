// Tests for LocalInterfaces (lib/src/services/local_interfaces.dart) ——
// 局域网地址展示用的网卡排序。排序是纯函数，用三平台真实的接口名 / 地址组合
// 覆盖：物理网卡压过虚拟网卡、私网段优先级、同档保序、不丢项，以及真机枚举在
// 当前宿主上能跑通且结果自洽。

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/services/local_interfaces.dart';

LocalInterface iface(String name, String ip) =>
    LocalInterface(name: name, ip: ip);

void main() {
  group('LocalInterfaces.isLikelyVirtual', () {
    test('flags virtual/tunnel adapters on all three desktop platforms', () {
      // Windows
      expect(LocalInterfaces.isLikelyVirtual('vEthernet (WSL)'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('vEthernet (Default Switch)'), isTrue);
      expect(
        LocalInterfaces.isLikelyVirtual('VMware Network Adapter VMnet8'),
        isTrue,
      );
      expect(
        LocalInterfaces.isLikelyVirtual('VirtualBox Host-Only Network'),
        isTrue,
      );
      expect(LocalInterfaces.isLikelyVirtual('ZeroTier One [abc]'), isTrue);
      // Linux
      expect(LocalInterfaces.isLikelyVirtual('docker0'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('br-1a2b3c'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('virbr0'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('veth9f2'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('tun0'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('wg0'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('tailscale0'), isTrue);
      // macOS
      expect(LocalInterfaces.isLikelyVirtual('utun3'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('bridge100'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('awdl0'), isTrue);
      expect(LocalInterfaces.isLikelyVirtual('vmnet1'), isTrue);
    });

    test('does not flag real adapters, including localized Windows names', () {
      expect(LocalInterfaces.isLikelyVirtual('WLAN'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('以太网'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('以太网 2'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('Ethernet'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('Wi-Fi'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('en0'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('en1'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('eth0'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('wlan0'), isFalse);
      expect(LocalInterfaces.isLikelyVirtual('enp3s0'), isFalse);
    });
  });

  group('LocalInterfaces.sortForLan', () {
    test('Windows: real LAN adapter wins over Hyper-V/WSL switches', () {
      final sorted = LocalInterfaces.sortForLan([
        iface('以太网 2', '2.0.0.1'),
        iface('以太网', '172.32.8.100'),
        iface('WLAN', '192.168.1.100'),
        iface('vEthernet (Default Switch)', '172.31.208.1'),
        iface('vEthernet (WSL)', '172.28.224.1'),
      ]);
      expect(sorted.first.ip, '192.168.1.100');
      // 虚拟适配器整体退到所有物理网卡之后，哪怕它的网段更"私有"。
      expect(sorted.map((e) => e.name).toList(), [
        'WLAN',
        '以太网 2',
        '以太网',
        'vEthernet (Default Switch)',
        'vEthernet (WSL)',
      ]);
    });

    test('Linux: wlan0 beats docker0/virbr0 even on the same /16 family', () {
      final sorted = LocalInterfaces.sortForLan([
        iface('virbr0', '192.168.122.1'),
        iface('docker0', '172.17.0.1'),
        iface('wlan0', '192.168.1.7'),
        iface('tailscale0', '100.101.102.103'),
      ]);
      expect(sorted.map((e) => e.name).toList(), [
        'wlan0',
        'virbr0',
        'docker0',
        'tailscale0',
      ]);
    });

    test('macOS: en0 beats bridge100/utun tunnels', () {
      final sorted = LocalInterfaces.sortForLan([
        iface('utun4', '10.2.0.5'),
        iface('bridge100', '192.168.64.1'),
        iface('en0', '10.0.1.23'),
      ]);
      expect(sorted.map((e) => e.name).toList(), ['en0', 'bridge100', 'utun4']);
    });

    test('private subnet precedence is 192.168 > 10 > 172.16/12 > other', () {
      final sorted = LocalInterfaces.sortForLan([
        iface('a', '203.0.113.9'),
        iface('b', '172.20.5.5'),
        iface('c', '10.4.4.4'),
        iface('d', '192.168.8.8'),
      ]);
      expect(sorted.map((e) => e.ip).toList(), [
        '192.168.8.8',
        '10.4.4.4',
        '172.20.5.5',
        '203.0.113.9',
      ]);
    });

    test('172.32/172.15 are not private and rank after 172.16-31', () {
      final sorted = LocalInterfaces.sortForLan([
        iface('a', '172.32.0.1'),
        iface('b', '172.15.0.1'),
        iface('c', '172.16.0.1'),
        iface('d', '172.31.255.1'),
      ]);
      expect(sorted.map((e) => e.ip).take(2).toList(), [
        '172.16.0.1',
        '172.31.255.1',
      ]);
    });

    test('same rank keeps OS enumeration order (stable)', () {
      final input = [
        iface('eth0', '192.168.1.2'),
        iface('eth1', '192.168.1.3'),
        iface('eth2', '192.168.1.4'),
      ];
      expect(LocalInterfaces.sortForLan(input), input);
    });

    test('never drops or duplicates entries', () {
      final input = [
        iface('docker0', '172.17.0.1'),
        iface('en0', '192.168.0.2'),
        iface('utun0', '100.64.0.1'),
        iface('eth0', '10.1.1.1'),
      ];
      final sorted = LocalInterfaces.sortForLan(input);
      expect(sorted.length, input.length);
      expect(sorted.toSet(), input.toSet());
    });

    test('empty input yields empty output', () {
      expect(LocalInterfaces.sortForLan(const []), isEmpty);
    });
  });

  group('LocalInterfaces.list', () {
    test('enumerates the host without throwing and never returns loopback', () async {
      final ips = await LocalInterfaces.list();
      for (final e in ips) {
        expect(e.ip, isNot(LocalInterfaces.loopback));
        expect(e.ip.startsWith('127.'), isFalse);
        expect(e.ip.startsWith('169.254.'), isFalse, reason: 'link-local excluded');
        expect(e.ip.split('.').length, 4, reason: 'IPv4 only');
        expect(e.name, isNotEmpty);
      }
      // 已排序：任意相邻两项的权重非递减。
      for (var i = 1; i < ips.length; i++) {
        expect(
          LocalInterfaces.rank(ips[i - 1]) <= LocalInterfaces.rank(ips[i]),
          isTrue,
        );
      }
    });
  });
}
