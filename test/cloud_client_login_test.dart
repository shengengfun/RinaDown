import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:flux_down/src/services/cloud/cloud_client.dart';
import 'package:flux_down/src/services/cloud/cloud_models.dart';
import 'package:flux_down/src/services/kv_store.dart';

void main() {
  late Directory tmp;
  late HttpServer server;

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('cloud_client_login_test');
    KvStore.instance.debugReset();
    KvStore.instance.debugInitPortable(
      File('${tmp.path}${Platform.pathSeparator}settings.json'),
    );

    server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    server.listen((request) async {
      if (request.uri.path != '/api/v1/auth/login') {
        request.response.statusCode = HttpStatus.notFound;
        await request.response.close();
        return;
      }
      final body = jsonDecode(await utf8.decoder.bind(request).join());
      final account = (body as Map<String, dynamic>)['account'];
      request.response
        ..statusCode = HttpStatus.ok
        ..headers.contentType = ContentType.json
        ..write(
          jsonEncode({
            'status': 'deviceVerificationRequired',
            'ttlSeconds': 600,
            if (account == 'replacement@example.com')
              'willReplaceDevices': true,
          }),
        );
      await request.response.close();
    });

    await CloudApiConfig.setBaseUrl('http://127.0.0.1:${server.port}');
    CloudClient.instance
      ..accessToken = null
      ..refreshToken = null;
  });

  tearDown(() async {
    await server.close(force: true);
    CloudClient.instance
      ..accessToken = null
      ..refreshToken = null;
    KvStore.instance.debugReset();
    tmp.deleteSync(recursive: true);
  });

  test('设备验证响应提示替换，并兼容旧服务端缺省字段', () async {
    final replacement = await CloudClient.instance.login(
      account: 'replacement@example.com',
      password: 'password',
      deviceId: 'device-new',
    );
    final legacy = await CloudClient.instance.login(
      account: 'legacy@example.com',
      password: 'password',
      deviceId: 'device-new',
    );

    expect(
      replacement,
      isA<LoginDeviceVerificationRequired>()
          .having((result) => result.ttlSeconds, 'ttlSeconds', 600)
          .having(
            (result) => result.willReplaceDevices,
            'willReplaceDevices',
            isTrue,
          ),
    );
    expect(
      legacy,
      isA<LoginDeviceVerificationRequired>().having(
        (result) => result.willReplaceDevices,
        'willReplaceDevices',
        isFalse,
      ),
    );
  });
}
