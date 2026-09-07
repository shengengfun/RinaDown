// CloudClient 401 单飞刷新回归测试（issue #228「应用内更新后掉登录」）。
//
// 契约：
// 1. 并发多路 401 只允许发出一次 /auth/refresh（refreshToken 轮换制，旧 RT
//    刷新成功即作废——并发竞争刷新会把输掉竞态的调用误判成会话过期）；
// 2. 刷新遭遇网络级/暂时性故障（5xx）不得清空本地会话；
// 3. 刷新被服务端明确拒绝（401）才触发 onSessionExpired，且只触发一次。
//
// 用真实 dart:io HttpServer 充当 FluxCloud 桩：CloudApiConfig 在 debug 构建
// （flutter test 恒为 debug）下允许经 KvStore 覆盖 baseUrl。

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:flux_down/src/services/cloud/cloud_client.dart';
import 'package:flux_down/src/services/cloud/cloud_models.dart';
import 'package:flux_down/src/services/kv_store.dart';

const _stale = 'stale-access-token';
const _fresh = 'fresh-access-token';
const _rt1 = 'refresh-token-1';
const _rt2 = 'refresh-token-2';

Map<String, dynamic> _authResponseJson() => {
  'accessToken': _fresh,
  'refreshToken': _rt2,
  'expiresIn': 900,
  'user': {'id': 'u1', 'email': 'u@example.com'},
  'device': {'id': 'd1', 'deviceId': 'dev-1'},
};

Map<String, dynamic> _profileJson() => {
  'id': 'u1',
  'email': 'u@example.com',
  'entitlements': <String, dynamic>{},
};

void main() {
  late Directory tmp;
  late HttpServer server;
  late int refreshHits;
  late int sessionExpiredCalls;

  /// /auth/refresh 的行为，逐用例定制。
  late Future<void> Function(HttpRequest req) onRefresh;

  Future<void> ok(HttpRequest req, Map<String, dynamic> body) async {
    req.response
      ..statusCode = 200
      ..headers.contentType = ContentType.json
      ..write(jsonEncode(body));
    await req.response.close();
  }

  Future<void> status(HttpRequest req, int code) async {
    req.response
      ..statusCode = code
      ..headers.contentType = ContentType.json
      ..write(jsonEncode({'code': 'err_$code', 'message': 'HTTP $code'}));
    await req.response.close();
  }

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('cloud_client_test');
    KvStore.instance.debugReset();
    KvStore.instance.debugInitPortable(
      File('${tmp.path}${Platform.pathSeparator}settings.json'),
    );

    refreshHits = 0;
    sessionExpiredCalls = 0;

    server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    server.listen((req) async {
      final path = req.uri.path;
      if (path == '/api/v1/auth/refresh') {
        refreshHits++;
        await onRefresh(req);
        return;
      }
      if (path == '/api/v1/me') {
        final auth = req.headers.value(HttpHeaders.authorizationHeader);
        if (auth == 'Bearer $_fresh') {
          await ok(req, _profileJson());
        } else {
          await status(req, 401);
        }
        return;
      }
      await status(req, 404);
    });

    await CloudApiConfig.setBaseUrl('http://127.0.0.1:${server.port}');
    final client = CloudClient.instance
      ..accessToken = _stale
      ..refreshToken = _rt1;
    client.onTokenRefreshed = null;
    client.onSessionExpired = () => sessionExpiredCalls++;
  });

  tearDown(() async {
    await server.close(force: true);
    CloudClient.instance
      ..accessToken = null
      ..refreshToken = null
      ..onTokenRefreshed = null
      ..onSessionExpired = null;
    KvStore.instance.debugReset();
    tmp.deleteSync(recursive: true);
  });

  test('并发 401 共享一次刷新，全部重放成功且不清会话', () async {
    onRefresh = (req) async {
      final body = jsonDecode(await utf8.decodeStream(req)) as Map<String, dynamic>;
      // 轮换制：只认第一次的 rt1，旧 RT 复用一律拒绝。
      if (refreshHits == 1 && body['refreshToken'] == _rt1) {
        await ok(req, _authResponseJson());
      } else {
        await status(req, 401);
      }
    };

    final results = await Future.wait(
      List.generate(4, (_) => CloudClient.instance.me()),
    );

    expect(results, hasLength(4));
    expect(refreshHits, 1, reason: '并发 401 必须共享同一次 /auth/refresh');
    expect(sessionExpiredCalls, 0);
    expect(CloudClient.instance.accessToken, _fresh);
    expect(CloudClient.instance.refreshToken, _rt2);
  });

  test('刷新遇 5xx（暂时性故障）不清会话，抛出原始 401', () async {
    onRefresh = (req) => status(req, 503);

    await expectLater(
      CloudClient.instance.me(),
      throwsA(
        isA<CloudApiException>().having((e) => e.status, 'status', 401),
      ),
    );
    expect(sessionExpiredCalls, 0, reason: '网络级/5xx 刷新失败不得登出');
    // 令牌保持原样，下次重试仍可用。
    expect(CloudClient.instance.refreshToken, _rt1);
  });

  test('刷新被服务端明确拒绝（401）才清会话', () async {
    onRefresh = (req) => status(req, 401);

    await expectLater(
      CloudClient.instance.me(),
      throwsA(isA<CloudApiException>()),
    );
    expect(sessionExpiredCalls, 1);
  });
}
