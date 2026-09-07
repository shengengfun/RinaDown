import 'package:flutter_test/flutter_test.dart';

import 'package:flux_down/src/models/site_auth_store.dart';

void main() {
  group('siteKeyFromUrl', () {
    test('lowercases host, drops path/query', () {
      expect(
        siteKeyFromUrl('https://Example.COM/some/File.zip?a=1'),
        'example.com',
      );
    });

    test('explicit default port is excluded', () {
      expect(siteKeyFromUrl('http://example.com:80/f'), 'example.com');
      expect(siteKeyFromUrl('https://example.com:443/f'), 'example.com');
    });

    test('explicit non-default port is kept', () {
      expect(siteKeyFromUrl('http://example.com:8080/f'), 'example.com:8080');
      expect(siteKeyFromUrl('https://example.com:80/f'), 'example.com:80');
      expect(siteKeyFromUrl('http://example.com:443/f'), 'example.com:443');
    });

    test('non-http(s) or invalid returns null', () {
      expect(siteKeyFromUrl('ftp://example.com/f'), isNull);
      expect(siteKeyFromUrl('magnet:?xt=urn:btih:abc'), isNull);
      expect(siteKeyFromUrl(''), isNull);
      expect(siteKeyFromUrl('https://'), isNull);
    });
  });

  group('parseSiteAuthStore', () {
    test('parses valid table', () {
      final map = parseSiteAuthStore(
        '{"example.com":{"user":"u","pass":"p"},'
        '"host:8080":{"user":"a","pass":"b"}}',
      );
      expect(map['example.com'], (user: 'u', pass: 'p'));
      expect(map['host:8080'], (user: 'a', pass: 'b'));
    });

    test('tolerates empty, corrupt, and malformed entries', () {
      expect(parseSiteAuthStore(''), isEmpty);
      expect(parseSiteAuthStore('  '), isEmpty);
      expect(parseSiteAuthStore('not json'), isEmpty);
      expect(parseSiteAuthStore('[1,2]'), isEmpty);
      final map = parseSiteAuthStore(
        '{"bad":"str","ok":{"user":"u","pass":"p"},"partial":{}}',
      );
      expect(map.containsKey('bad'), isFalse);
      expect(map['ok'], (user: 'u', pass: 'p'));
      expect(map['partial'], (user: '', pass: ''));
    });
  });

  group('filterSiteAuth', () {
    final store = <String, ({String user, String pass})>{
      'nas.example.com:8443': (user: 'admin', pass: 'p1'),
      'files.acme.org': (user: 'bob', pass: 'p2'),
      'media.acme.org': (user: 'Alice', pass: 'p3'),
    };

    List<String> keys(String query) =>
        filterSiteAuth(store, query).map((e) => e.key).toList();

    test('empty query returns all entries in site order', () {
      expect(keys(''), [
        'files.acme.org',
        'media.acme.org',
        'nas.example.com:8443',
      ]);
      expect(keys('   '), hasLength(3));
    });

    test('matches site substring case-insensitively', () {
      expect(keys('ACME'), ['files.acme.org', 'media.acme.org']);
      expect(keys('8443'), ['nas.example.com:8443']);
    });

    test('matches username case-insensitively', () {
      expect(keys('alice'), ['media.acme.org']);
      expect(keys('admin'), ['nas.example.com:8443']);
    });

    test('every whitespace-separated term must match', () {
      expect(keys('acme bob'), ['files.acme.org']);
      // 两词分别命中不同条目 → 交集为空
      expect(keys('acme admin'), isEmpty);
    });

    test('no match yields empty list', () {
      expect(keys('nonexistent'), isEmpty);
    });
  });
}
