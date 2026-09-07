// Tests for NicknamePool (lib/src/services/cloud/nickname_pool.dart) ——
// 纯函数，覆盖：同 seed 可复现、中英文拼接格式、词库中英下标对齐、无 seed 时有随机性。

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/services/cloud/nickname_pool.dart';

void main() {
  group('NicknamePool.suggest', () {
    test('the same seed always reproduces the same nickname', () {
      final a = NicknamePool.suggest(true, seed: 42);
      final b = NicknamePool.suggest(true, seed: 42);
      expect(a, b);

      final c = NicknamePool.suggest(false, seed: 7);
      final d = NicknamePool.suggest(false, seed: 7);
      expect(c, d);
    });

    test('zh nicknames concatenate adjective + animal with no separator', () {
      final n = NicknamePool.suggest(true, seed: 1);
      expect(n.contains(' '), isFalse);
      expect(NicknamePool.adjectivesZh.any(n.startsWith), isTrue);
      expect(NicknamePool.animalsZh.any(n.endsWith), isTrue);
    });

    test('en nicknames are "Adj Animal" separated by exactly one space', () {
      final n = NicknamePool.suggest(false, seed: 1);
      final parts = n.split(' ');
      expect(parts.length, 2);
      expect(NicknamePool.adjectivesEn, contains(parts[0]));
      expect(NicknamePool.animalsEn, contains(parts[1]));
    });

    test('zh/en word lists stay index-aligned for the same underlying meaning', () {
      expect(NicknamePool.adjectivesZh.length, NicknamePool.adjectivesEn.length);
      expect(NicknamePool.animalsZh.length, NicknamePool.animalsEn.length);
    });

    test('without a seed, repeated suggestions are not always identical', () {
      final samples = List.generate(30, (_) => NicknamePool.suggest(true));
      expect(samples.toSet().length, greaterThan(1));
    });
  });
}
