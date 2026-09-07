import 'package:flutter_test/flutter_test.dart';

import 'package:flux_down/src/widgets/update_changelog_dialog.dart';

void main() {
  const bilingual = '''
<!-- fluxdown:lang:zh -->
## 0.4.4 - 2026-08-14

### 🚀 新功能

- 下载列表支持批量选择

<!-- fluxdown:lang:en -->
## 0.4.4 - 2026-08-14

### 🚀 Features

- The download list now supports batch selection
''';

  group('pickLocaleBody', () {
    test('zh locale gets only the zh section, markers stripped', () {
      final body = pickLocaleBody(bilingual, 'zh');
      expect(body, contains('新功能'));
      expect(body, isNot(contains('Features')));
      expect(body, isNot(contains('fluxdown:lang')));
    });

    test('en locale gets only the en section', () {
      final body = pickLocaleBody(bilingual, 'en');
      expect(body, contains('Features'));
      expect(body, isNot(contains('新功能')));
      expect(body, isNot(contains('fluxdown:lang')));
    });

    test('zh regional variants resolve to zh section', () {
      expect(pickLocaleBody(bilingual, 'zh-CN'), contains('新功能'));
    });

    test('unknown locale falls back to zh section', () {
      // 与官网 ChangelogSection.tsx 契约一致：非 zh 前缀按 en 取，
      // en 区块缺失时回退 zh。
      const zhOnly = '<!-- fluxdown:lang:zh -->\n- 仅中文条目\n';
      expect(pickLocaleBody(zhOnly, 'ja'), '- 仅中文条目');
    });

    test('body without markers is returned unchanged', () {
      const legacy = '## 0.3.0\n\n- legacy release notes';
      expect(pickLocaleBody(legacy, 'zh'), legacy);
      expect(pickLocaleBody(legacy, 'en'), legacy);
    });
  });
}
