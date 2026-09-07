// FluxCloud 客户端 —— 轻量 JSON HTTP 封装（同 feedback_service.dart 用法：
// dart:io HttpClient，不引入 http 包），严格实现契约 v1 全部客户端接口。
//
// 401 自动刷新：devices/me 等需要 Bearer 的接口若返回 401，经单飞刷新用
// refreshToken 换新令牌并重放原请求；刷新被服务端明确拒绝（refreshToken
// 过期/被吊销）则触发 [onSessionExpired]，由 CloudAuthService 清空本地会话。
// 本文件只负责传输层
// 机制，不持久化任何令牌 —— 令牌的读取/持久化由 CloudAuthService 通过
// [accessToken]/[refreshToken] 字段与 [onTokenRefreshed] 回调完成同步。

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../kv_store.dart';
import 'cloud_models.dart';
import 'device_identity.dart';

/// 默认服务地址：Actions 打包时用 --dart-define=FLUXCLOUD_BASE_URL=https://... 注入
/// 官方地址，未注入时回退本地联调端口（开发期）。
const _kDefaultApiBase = String.fromEnvironment(
  'FLUXCLOUD_BASE_URL',
  defaultValue: 'http://127.0.0.1:8720',
);
const _kApiBaseKvKey = 'cloud_api_base';
const _kApiPrefix = '/api/v1';

/// FluxCloud 服务地址配置：默认常量 + kv_store 覆盖项，供设置页读写。
class CloudApiConfig {
  CloudApiConfig._();

  /// 官方默认地址（开发期指向本地联调端口）。
  static const String defaultBaseUrl = _kDefaultApiBase;

  /// 当前生效的服务地址：仅调试构建允许 kv 自定义覆盖（对应设置项也只在
  /// 调试构建显示），正式包锁定默认常量，避免残留覆盖值指向失效地址。
  static String get baseUrl {
    if (!kDebugMode) return _kDefaultApiBase;
    final custom = KvStore.instance.getString(_kApiBaseKvKey);
    return (custom != null && custom.trim().isNotEmpty)
        ? custom.trim()
        : _kDefaultApiBase;
  }

  /// 是否为用户自定义地址（非默认值），供设置页展示"恢复默认"按钮状态。
  static bool get isCustom {
    final custom = KvStore.instance.getString(_kApiBaseKvKey);
    return custom != null &&
        custom.trim().isNotEmpty &&
        custom.trim() != _kDefaultApiBase;
  }

  static Future<void> setBaseUrl(String url) =>
      KvStore.instance.setString(_kApiBaseKvKey, url.trim());

  static Future<void> resetToDefault() =>
      KvStore.instance.remove(_kApiBaseKvKey);
}

class CloudClient {
  CloudClient._();
  static final CloudClient instance = CloudClient._();

  static const _timeout = Duration(seconds: 15);

  /// 当前会话令牌，由 CloudAuthService 在登录/刷新/登出时同步写入。
  /// 客户端只用它们发起带 Authorization 头的请求 + 401 时的刷新重放，
  /// 不持久化、不感知具体业务状态。
  String? accessToken;
  String? refreshToken;

  /// 401 触发的刷新流程成功后回调，供上层持久化新令牌 + 更新用户快照。
  void Function(AuthResponse auth)? onTokenRefreshed;

  /// 刷新也失败（refreshToken 过期/被吊销）时回调，供上层清空本地会话。
  void Function()? onSessionExpired;

  /// 在途的单飞刷新任务；并发 401 共享同一次轮换（见 [_refreshSingleFlight]）。
  Future<void>? _refreshInFlight;

  HttpClient? _http;

  void _ensureHttp() {
    _http ??= HttpClient()
      ..connectionTimeout = const Duration(seconds: 10)
      ..idleTimeout = const Duration(seconds: 15);
  }

  /// 释放底层连接池；本服务为应用级单例，正常不需要主动调用。
  void dispose() {
    _http?.close(force: true);
    _http = null;
  }

  // ── 注册 / 登录 ──────────────────────────────────────────────────────

  /// POST /auth/register：发码建 pending 用户，返回验证码 TTL（秒）。
  Future<int> register({
    required String email,
    required String password,
    String? nickname,
  }) async {
    final json = await _request(
      'POST',
      '/auth/register',
      body: {
        'email': email,
        'password': password,
        if (nickname != null && nickname.trim().isNotEmpty)
          'nickname': nickname.trim(),
      },
    );
    return _ttlSeconds(json);
  }

  /// POST /auth/register/verify：验证码激活 pending 用户 + 信任当前设备 + 签发令牌。
  Future<AuthResponse> registerVerify({
    required String email,
    required String code,
    required String deviceId,
    String? deviceName,
    String? devicePlatform,
    String? appVersion,
  }) async {
    final json = await _request(
      'POST',
      '/auth/register/verify',
      body: _withDeviceInfo(
        {'email': email, 'code': code},
        deviceId,
        deviceName,
        devicePlatform,
        appVersion,
      ),
    );
    return AuthResponse.fromJson(json);
  }

  /// POST /auth/login：tagged 响应，设备已受信任直接下发令牌；
  /// 新设备返回 deviceVerificationRequired，服务端自动发码，并可预告是否会替换旧设备。
  /// [account] 接受邮箱或纯数字 Origin ID（契约 v1.2），服务端字段名 account。
  Future<LoginResult> login({
    required String account,
    required String password,
    required String deviceId,
    String? deviceName,
    String? devicePlatform,
    String? appVersion,
  }) async {
    final json = await _request(
      'POST',
      '/auth/login',
      body: _withDeviceInfo(
        {'account': account, 'password': password},
        deviceId,
        deviceName,
        devicePlatform,
        appVersion,
      ),
    );
    final status = json['status'] as String?;
    if (status == 'deviceVerificationRequired') {
      return LoginDeviceVerificationRequired(
        _ttlSeconds(json),
        willReplaceDevices: json['willReplaceDevices'] == true,
      );
    }
    final authJson = json['auth'];
    if (status == 'ok' && authJson is Map<String, dynamic>) {
      return LoginOk(AuthResponse.fromJson(authJson));
    }
    throw const CloudApiException(
      code: 'malformed_response',
      message: '登录响应格式异常',
      status: 200,
    );
  }

  /// POST /auth/login/verify：新设备验证码登录，重新校验密码 + 消费验证码。
  /// [account] 语义同 [login]。
  Future<AuthResponse> loginVerify({
    required String account,
    required String password,
    required String code,
    required String deviceId,
    String? deviceName,
    String? devicePlatform,
    String? appVersion,
  }) async {
    final json = await _request(
      'POST',
      '/auth/login/verify',
      body: _withDeviceInfo(
        {'account': account, 'password': password, 'code': code},
        deviceId,
        deviceName,
        devicePlatform,
        appVersion,
      ),
    );
    return AuthResponse.fromJson(json);
  }

  /// POST /auth/code/send：发送验证码登录用的验证码，返回 TTL（秒）。
  Future<int> sendCode(String email) async {
    final json = await _request(
      'POST',
      '/auth/code/send',
      body: {'email': email},
    );
    return _ttlSeconds(json);
  }

  /// POST /auth/code/verify：验证码登录（邮箱不存在则自动注册），信任当前设备。
  /// [nickname] 仅在服务端"邮箱不存在→自动注册新用户"分支生效，已存在用户忽略，
  /// 可放心恒传（默认昵称跟随当前界面语言，见 nickname_pool.dart）。
  Future<AuthResponse> verifyCode({
    required String email,
    required String code,
    required String deviceId,
    String? deviceName,
    String? devicePlatform,
    String? appVersion,
    String? nickname,
  }) async {
    final json = await _request(
      'POST',
      '/auth/code/verify',
      body: _withDeviceInfo(
        {
          'email': email,
          'code': code,
          if (nickname != null && nickname.trim().isNotEmpty)
            'nickname': nickname.trim(),
        },
        deviceId,
        deviceName,
        devicePlatform,
        appVersion,
      ),
    );
    return AuthResponse.fromJson(json);
  }

  /// POST /auth/refresh：刷新令牌轮换。
  Future<AuthResponse> refresh(String refreshToken) async {
    final json = await _request(
      'POST',
      '/auth/refresh',
      body: {'refreshToken': refreshToken},
    );
    return AuthResponse.fromJson(json);
  }

  /// POST /auth/logout。
  Future<void> logout(String refreshToken) async {
    await _request(
      'POST',
      '/auth/logout',
      body: {'refreshToken': refreshToken},
    );
  }

  // ── 已登录接口（Bearer UserAuth，401 自动刷新重放一次）──────────────────

  /// GET /me：当前用户信息 + 套餐能力快照。
  Future<CloudProfile> me() => _authed(() async {
    final json = await _request('GET', '/me', authed: true);
    return CloudProfile.fromJson(json);
  });

  /// GET /devices：当前用户名下已信任设备，按 lastSeenAt 降序。
  Future<List<CloudDevice>> devices() => _authed(() async {
    final json = await _request(
      'GET',
      '/devices?deviceId=${Uri.encodeQueryComponent(DeviceIdentity.deviceId())}',
      authed: true,
    );
    final list = json['devices'] as List<dynamic>? ?? const [];
    return list
        .map((e) => CloudDevice.fromJson(e as Map<String, dynamic>))
        .toList();
  });

  /// PATCH /devices/{id} {name}：设备改名，1-64 字符校验由服务端兜底。
  Future<CloudDevice> renameDevice(String id, String name) => _authed(() async {
    final json = await _request(
      'PATCH',
      '/devices/$id',
      body: {'name': name},
      authed: true,
    );
    return CloudDevice.fromJson(json);
  });

  /// DELETE /devices/{id}：删除设备 + 吊销其名下全部未撤销 refresh token。
  Future<void> deleteDevice(String id) => _authed(() async {
    await _request('DELETE', '/devices/$id', authed: true);
  });

  // ── 跨设备任务协同（Bearer UserAuth；SSE 事件流由 RemoteTaskService 独立直连）──

  /// POST /tasks/dispatch：把下载任务下发给目标设备执行。返回创建的跨设备任务。
  Future<RemoteTask> dispatchTask({
    required String toDevice,
    required String url,
    String? saveDir,
    String? fileName,
    Map<String, dynamic>? options,
  }) => _authed(() async {
    final json = await _request(
      'POST',
      '/tasks/dispatch',
      body: {
        'deviceId': DeviceIdentity.deviceId(),
        'toDevice': toDevice,
        'url': url,
        if (saveDir != null && saveDir.isNotEmpty) 'saveDir': saveDir,
        if (fileName != null && fileName.isNotEmpty) 'fileName': fileName,
        ...?options,
      },
      authed: true,
    );
    return RemoteTask.fromJson(json);
  });

  /// GET /tasks/remote：拉取本账号全部跨设备任务（持久态 + 内存进度快照），断线重连用。
  Future<List<RemoteTask>> remoteTasks() => _authed(() async {
    final json = await _request('GET', '/tasks/remote', authed: true);
    final list = json['tasks'] as List<dynamic>? ?? const [];
    return list
        .map((e) => RemoteTask.fromJson(e as Map<String, dynamic>))
        .toList();
  });

  /// POST /tasks/{id}/status：执行端上报任务状态转换（服务端落库 + 广播）。
  Future<void> reportTaskStatus(
    String id, {
    required String status,
    int? totalBytes,
    String? fileName,
    String? error,
  }) => _authed(() async {
    await _request(
      'POST',
      '/tasks/$id/status',
      body: {
        'status': status,
        'totalBytes': ?totalBytes,
        if (fileName != null && fileName.isNotEmpty) 'fileName': fileName,
        if (error != null && error.isNotEmpty) 'error': error,
      },
      authed: true,
    );
  });

  /// POST /tasks/progress：执行端批量上报进度（服务端仅更内存 + 广播，不落库）。
  Future<void> reportProgress(List<ProgressReport> items) => _authed(() async {
    if (items.isEmpty) return;
    await _request(
      'POST',
      '/tasks/progress',
      body: {'items': items.map((e) => e.toJson()).toList()},
      authed: true,
    );
  });

  /// POST /tasks/{id}/command：向执行端下发控制命令（pause/resume/cancel）。
  Future<void> commandTask(String id, String action) => _authed(() async {
    await _request(
      'POST',
      '/tasks/$id/command',
      body: {'action': action},
      authed: true,
    );
  });

  /// POST /tasks/presence：维持本设备到本账号 presence 租约的心跳（C1+C2
  /// 契约）。无请求体，成功 204；调用方（RemoteTaskService）在维持 SSE
  /// 长连期间每 30s 调用一次续租，服务端 90s 未收到心跳会判定离线。
  Future<void> pingPresence() => _authed(() async {
    await _request('POST', '/tasks/presence', authed: true);
  });

  /// POST /me/email/code：向当前绑定邮箱发送验证码（邮箱变更第一步），返回 TTL（秒）。
  Future<int> sendEmailChangeCode() => _authed(() async {
    final json = await _request('POST', '/me/email/code', authed: true);
    return _ttlSeconds(json);
  });

  /// POST /me/email/code/new：携原邮箱验证码向新邮箱发送验证码（第二步），返回 TTL（秒）。
  Future<int> sendEmailChangeNewCode({
    required String newEmail,
    required String oldCode,
  }) => _authed(() async {
    final json = await _request(
      'POST',
      '/me/email/code/new',
      body: {'email': newEmail, 'code': oldCode},
      authed: true,
    );
    return _ttlSeconds(json);
  });

  /// POST /me/email：同时校验原/新邮箱验证码并更新绑定邮箱（第三步），返回最新用户资料。
  Future<CloudProfile> changeEmail({
    required String newEmail,
    required String oldCode,
    required String newCode,
  }) => _authed(() async {
    final json = await _request(
      'POST',
      '/me/email',
      body: {'email': newEmail, 'oldCode': oldCode, 'newCode': newCode},
      authed: true,
    );
    return CloudProfile.fromJson(json);
  });

  // ── Origin ID 自助修改（v1.3 新增，见契约 GET/PUT /me/origin-id）───────

  /// GET /me/origin-id/random：随机生成一个建议 Origin ID（"豹子号"友好，不锁定，
  /// 仅供输入框预填，最终是否可用仍以提交时服务端裁决为准）。
  Future<int> randomOriginId() => _authed(() async {
    final json = await _request('GET', '/me/origin-id/random', authed: true);
    return (json['originId'] as num).toInt();
  });

  /// GET /me/origin-id/check?value=：查询指定 Origin ID 是否可用（提交前预检）。
  Future<OriginIdCheckResult> checkOriginId(int value) => _authed(() async {
    final json = await _request(
      'GET',
      '/me/origin-id/check?value=$value',
      authed: true,
    );
    return OriginIdCheckResult.fromJson(json);
  });

  /// PUT /me/origin-id：提交新 Origin ID（≥10000 的整数，全局仅可成功一次），
  /// 成功后返回最新用户资料（同 GET /me 结构）。
  Future<CloudProfile> changeOriginId(int originId) => _authed(() async {
    final json = await _request(
      'PUT',
      '/me/origin-id',
      body: {'originId': originId},
      authed: true,
    );
    return CloudProfile.fromJson(json);
  });

  /// PUT /me/nickname：提交新昵称（1-32 字符，服务端 trim 后校验），
  /// 成功后返回最新用户资料（同 GET /me 结构）。
  Future<CloudProfile> changeNickname(String nickname) => _authed(() async {
    final json = await _request(
      'PUT',
      '/me/nickname',
      body: {'nickname': nickname},
      authed: true,
    );
    return CloudProfile.fromJson(json);
  });

  // ── 套餐 / 订单（微信 Native 扫码购买，见 local://pay-contract.md）───────

  /// GET /plans/catalog：公开无鉴权。含上架套餐，以及下架但仍官网展示的套餐。
  Future<List<CloudPlan>> getPlansCatalog() async {
    final json = await _request('GET', '/plans/catalog');
    final list = json['value'] as List<dynamic>? ?? const [];
    return list
        .map((e) => CloudPlan.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  /// POST /orders：创建订单（同用户同套餐已有未过期 pending 订单时服务端直接复用返回）。
  /// [referralCode] 非空时随下单一并校验/入库（服务端权威计算立减与归因）。
  Future<CloudOrder> createOrder(
    String planCode, {
    String? deviceId,
    String? referralCode,
  }) => _authed(() async {
    final json = await _request(
      'POST',
      '/orders',
      body: {
        'planCode': planCode,
        if (deviceId != null && deviceId.isNotEmpty) 'deviceId': deviceId,
        if (referralCode != null && referralCode.isNotEmpty)
          'referralCode': referralCode,
      },
      authed: true,
    );
    return CloudOrder.fromJson(json);
  });

  /// GET /orders/{orderNo}：查询单个订单（仅本人），支付期间每 2s 轮询用。
  Future<CloudOrder> getOrder(String orderNo) => _authed(() async {
    final json = await _request(
      'GET',
      '/orders/${Uri.encodeComponent(orderNo)}',
      authed: true,
    );
    return CloudOrder.fromJson(json);
  });

  /// GET /orders：本人订单列表，按创建时间倒序，最多 20 条。
  Future<List<CloudOrder>> listOrders() => _authed(() async {
    final json = await _request('GET', '/orders', authed: true);
    final list = json['value'] as List<dynamic>? ?? const [];
    return list
        .map((e) => CloudOrder.fromJson(e as Map<String, dynamic>))
        .toList();
  });

  // ── 推介有奖（Bearer UserAuth，401 自动刷新重放一次）───────────────────

  /// GET /referral/summary：收益总览 + 说明文案 + 生效规则表；多码化后不再
  /// 随 summary 下发单一推荐码，见 [getReferralCodes]。
  Future<CloudReferralSummary> getReferralSummary() => _authed(() async {
    final json = await _request('GET', '/referral/summary', authed: true);
    return CloudReferralSummary.fromJson(json);
  });

  /// GET /referral/codes：我名下的推荐码列表，按创建时间升序，分页返回
  /// （[pageSize] 服务端上限 100；单用户码数上限另受创建接口 10 个约束）。
  Future<CloudReferralCodesResult> getReferralCodes({
    int page = 1,
    int pageSize = 20,
  }) => _authed(() async {
    final json = await _request(
      'GET',
      '/referral/codes?page=$page&pageSize=$pageSize',
      authed: true,
    );
    return CloudReferralCodesResult.fromJson(json);
  });

  /// POST /referral/codes：创建一个推荐码；[code] 为空时服务端随机生成 8 位。
  Future<CloudReferralCode> createReferralCode({String? code}) => _authed(
    () async {
      final json = await _request(
        'POST',
        '/referral/codes',
        body: {if (code != null && code.trim().isNotEmpty) 'code': code.trim()},
        authed: true,
      );
      return CloudReferralCode.fromJson(json);
    },
  );

  /// DELETE /referral/codes/{id}：删除我名下的推荐码（历史订单归因不受影响）。
  Future<void> deleteReferralCode(String id) => _authed(() async {
    await _request('DELETE', '/referral/codes/$id', authed: true);
  });

  /// GET /referral/records：我作为推荐人产生的返利台账，按创建时间倒序分页；
  /// [search] 非空时按买家昵称/邮箱大小写不敏感子串过滤（trim 后为空则不下发）。
  Future<CloudReferralRecordsResult> getReferralRecords({
    int page = 1,
    int pageSize = 20,
    String? search,
  }) => _authed(() async {
    final trimmedSearch = search?.trim() ?? '';
    final query = StringBuffer('page=$page&pageSize=$pageSize');
    if (trimmedSearch.isNotEmpty) {
      query.write('&search=${Uri.encodeQueryComponent(trimmedSearch)}');
    }
    final json = await _request(
      'GET',
      '/referral/records?$query',
      authed: true,
    );
    return CloudReferralRecordsResult.fromJson(json);
  });

  /// GET /referral/validate：下单前预校验推荐码对目标套餐是否可用 + 立减额。
  Future<CloudReferralValidateResult> validateReferralCode(
    String code,
    String planCode,
  ) => _authed(() async {
    final json = await _request(
      'GET',
      '/referral/validate?code=${Uri.encodeQueryComponent(code)}'
          '&planCode=${Uri.encodeQueryComponent(planCode)}',
      authed: true,
    );
    return CloudReferralValidateResult.fromJson(json);
  });

  // ── 配置同步（Bearer UserAuth，401 自动刷新重放一次；SSE 事件流由
  //    ConfigSyncService 用独立 HttpClient 直连，不走本类）──────────────────

  /// GET /sync/items：拉取 version > since 的条目（含墓碑），resync=true 时
  /// 客户端应重置水位线并将本地目录中云端缺失的键标脏重传。
  Future<SyncPullResult> syncPull({
    required int since,
    required String deviceId,
  }) => _authed(() async {
    final json = await _request(
      'GET',
      '/sync/items?since=$since&deviceId=${Uri.encodeQueryComponent(deviceId)}',
      authed: true,
    );
    return SyncPullResult.fromJson(json);
  });

  /// PUT /sync/items：批量推送本地变更，返回服务端最新 revision。回包 revision
  /// 恰为本地水位线+1 时，ConfigSyncService 会快进水位线以消除自回显 pull；
  /// 其余情况（有并发外部写入）仍靠 SSE 事件→pull 路径推进。
  Future<int> syncPush({
    required String deviceId,
    required List<Map<String, dynamic>> items,
  }) => _authed(() async {
    final json = await _request(
      'PUT',
      '/sync/items',
      body: {'deviceId': deviceId, 'items': items},
      authed: true,
    );
    return (json['revision'] as num?)?.toInt() ?? 0;
  });

  // ── CDN 聚合下载云端配置（Bearer UserAuth；ETag 条件请求，
  //    CdnConfigService 12h 周期 + 登录时拉取）────────────────────────────

  /// GET /cdn/config：P1 §四契约。不复用 [_request]——该端点需要发送
  /// If-None-Match 请求头、读取响应 ETag 头，且 304 是正常「未变更」结果
  /// 而非错误，与其余接口的错误语义不同。[ifNoneMatch] 传入上次持久化的
  /// ETag（原样回传，含引号）。
  Future<CdnConfigResult> fetchCdnConfig({String? ifNoneMatch}) => _authed(
    () async {
      _ensureHttp();
      final uri = Uri.parse('${CloudApiConfig.baseUrl}$_kApiPrefix/cdn/config');
      try {
        final req = await _http!.getUrl(uri).timeout(_timeout);
        req.headers.set('Accept', 'application/json');
        if (accessToken != null && accessToken!.isNotEmpty) {
          req.headers.set('Authorization', 'Bearer $accessToken');
        }
        if (ifNoneMatch != null && ifNoneMatch.isNotEmpty) {
          req.headers.set('If-None-Match', ifNoneMatch);
        }
        final res = await req.close().timeout(_timeout);
        if (res.statusCode == 304) {
          await res.drain<void>();
          return const CdnConfigResult.notModified();
        }
        final text = await res.transform(utf8.decoder).join();
        if (res.statusCode >= 200 && res.statusCode < 300) {
          final etag = res.headers.value(HttpHeaders.etagHeader);
          final json = text.trim().isEmpty
              ? const <String, dynamic>{}
              : jsonDecode(text) as Map<String, dynamic>;
          return CdnConfigResult(etag: etag, config: CdnConfig.fromJson(json));
        }
        var code = 'unknown_error';
        var message = 'HTTP ${res.statusCode}';
        try {
          final decoded = jsonDecode(text);
          if (decoded is Map<String, dynamic>) {
            code = (decoded['code'] as String?) ?? code;
            message = (decoded['message'] as String?) ?? message;
          }
        } catch (_) {
          // 错误体不是合法 JSON：保留默认 code/message，不阻断错误抛出。
        }
        throw CloudApiException(
          code: code,
          message: message,
          status: res.statusCode,
        );
      } on CloudApiException {
        rethrow;
      } on TimeoutException {
        throw const CloudApiException(
          code: 'network_error',
          message: '请求超时，请检查网络或服务器地址',
          status: 0,
        );
      } catch (e) {
        throw CloudApiException(
          code: 'network_error',
          message: '网络请求失败：$e',
          status: 0,
        );
      }
    },
  );

  // ── CDN 众包遥测上报（Bearer UserAuth；P2 §五契约，由 CdnReportService
  //    每 30min + 启动时上传引擎侧缓冲的 `cdn_pending_reports`）────────────

  /// POST /cdn/report：上报一批遥测样本。调用方须保证 [samples] ≤64 条
  /// （服务端单次批量上限），超量由调用方分批；样本元素形状与契约
  /// `samples[]` 一致（host/ip/connect_ms?/throughput_bps?/ok），
  /// `device_hash` 由服务端从鉴权设备 id 派生，本端不发送。成功 204。
  Future<void> reportCdnSamples(List<Map<String, dynamic>> samples) =>
      _authed(() async {
        if (samples.isEmpty) return;
        await _request(
          'POST',
          '/cdn/report',
          body: {'samples': samples},
          authed: true,
        );
      });

  // ── 内部实现 ─────────────────────────────────────────────────────────

  Map<String, dynamic> _withDeviceInfo(
    Map<String, dynamic> body,
    String deviceId,
    String? deviceName,
    String? devicePlatform,
    String? appVersion,
  ) => {
    ...body,
    'deviceId': deviceId,
    if (deviceName != null && deviceName.isNotEmpty) 'deviceName': deviceName,
    if (devicePlatform != null && devicePlatform.isNotEmpty)
      'devicePlatform': devicePlatform,
    if (appVersion != null && appVersion.isNotEmpty) 'appVersion': appVersion,
  };

  int _ttlSeconds(Map<String, dynamic> json) =>
      (json['ttlSeconds'] as num?)?.toInt() ?? 0;

  /// 需要 Bearer 认证的调用统一包装：命中 401 时经单飞刷新换新令牌后重放
  /// 一次；刷新失败则把原始 401 抛出去（会话是否清除由 [_doRefresh] 按
  /// 失败性质决定）。
  Future<T> _authed<T>(Future<T> Function() call) async {
    final usedToken = accessToken;
    try {
      return await call();
    } on CloudApiException catch (e) {
      if (e.status != 401) rethrow;
      try {
        await _refreshSingleFlight(staleAccessToken: usedToken);
      } catch (_) {
        // rethrow 只会重抛这个 catch 自己捕获的刷新失败异常；这里要的是原始 401，
        // 显式 throw e（闭包捕获外层 catch 绑定的异常）。
        throw e;
      }
      return await call();
    }
  }

  /// 单飞 token 刷新：同一时刻只允许一次 /auth/refresh 在途，并发 401 共享结果。
  ///
  /// 背景：refreshToken 是轮换制——刷新成功即作废旧 RT。冷启动时 access token
  /// 已过期，ConfigSync / RemoteTask / CdnConfig / CdnReport 多路并发首拉会
  /// 同时收到 401；若各自拿同一个旧 RT 竞争刷新，只有先到的成功，其余被
  /// 服务端拒绝，进而把「正常的并发刷新竞态」误判成「会话过期」清空本地
  /// 登录（用户表现为「应用内更新/重启后掉登录」，issue #228）。
  ///
  /// [staleAccessToken] 是调用方发起原请求时所持的 access token：与当前值
  /// 不同说明并发刷新已完成，直接返回用新令牌重放即可。
  Future<void> _refreshSingleFlight({required String? staleAccessToken}) async {
    if (accessToken != null && accessToken != staleAccessToken) return;
    final inflight = _refreshInFlight;
    if (inflight != null) return inflight;
    final rt = refreshToken;
    if (rt == null || rt.isEmpty) {
      onSessionExpired?.call();
      throw const CloudApiException(
        code: 'no_refresh_token',
        message: '无可用 refreshToken',
        status: 401,
      );
    }
    final task = _doRefresh(rt);
    _refreshInFlight = task;
    try {
      await task;
    } finally {
      _refreshInFlight = null;
    }
  }

  /// 执行一次令牌轮换并同步内存令牌 + 上层持久化。
  ///
  /// 只有服务端明确拒绝（401/403：refreshToken 过期、被吊销或复用检测命中）
  /// 才触发 [onSessionExpired] 清会话；网络错误（status 0）与 5xx 属暂时性
  /// 故障，保留本地会话交由调用方退避重试——离线启动不应把用户登出。
  Future<void> _doRefresh(String rt) async {
    try {
      final auth = await refresh(rt);
      accessToken = auth.accessToken;
      refreshToken = auth.refreshToken;
      onTokenRefreshed?.call(auth);
    } on CloudApiException catch (e) {
      if (e.status == 401 || e.status == 403) {
        onSessionExpired?.call();
      }
      rethrow;
    }
  }

  Future<Map<String, dynamic>> _request(
    String method,
    String path, {
    Map<String, dynamic>? body,
    bool authed = false,
  }) async {
    _ensureHttp();
    final uri = Uri.parse('${CloudApiConfig.baseUrl}$_kApiPrefix$path');
    try {
      final HttpClientRequest req = await switch (method) {
        'GET' => _http!.getUrl(uri).timeout(_timeout),
        'POST' => _http!.postUrl(uri).timeout(_timeout),
        'PUT' => _http!.putUrl(uri).timeout(_timeout),
        'PATCH' => _http!.patchUrl(uri).timeout(_timeout),
        'DELETE' => _http!.deleteUrl(uri).timeout(_timeout),
        _ => throw ArgumentError('unsupported method $method'),
      };
      req.headers.set('Accept', 'application/json');
      if (authed && accessToken != null && accessToken!.isNotEmpty) {
        req.headers.set('Authorization', 'Bearer $accessToken');
      }
      if (body != null) {
        final payload = utf8.encode(jsonEncode(body));
        req.headers.set('Content-Type', 'application/json; charset=utf-8');
        req.contentLength = payload.length;
        req.add(payload);
      }
      final res = await req.close().timeout(_timeout);
      final text = await res.transform(utf8.decoder).join();

      if (res.statusCode >= 200 && res.statusCode < 300) {
        if (text.trim().isEmpty) return const {};
        final decoded = jsonDecode(text);
        return decoded is Map<String, dynamic> ? decoded : {'value': decoded};
      }

      var code = 'unknown_error';
      var message = 'HTTP ${res.statusCode}';
      try {
        final decoded = jsonDecode(text);
        if (decoded is Map<String, dynamic>) {
          code = (decoded['code'] as String?) ?? code;
          message = (decoded['message'] as String?) ?? message;
        }
      } catch (_) {
        // 错误体不是合法 JSON：保留默认 code/message，不阻断错误抛出。
      }
      throw CloudApiException(
        code: code,
        message: message,
        status: res.statusCode,
      );
    } on CloudApiException {
      rethrow;
    } on TimeoutException {
      throw const CloudApiException(
        code: 'network_error',
        message: '请求超时，请检查网络或服务器地址',
        status: 0,
      );
    } catch (e) {
      throw CloudApiException(
        code: 'network_error',
        message: '网络请求失败：$e',
        status: 0,
      );
    }
  }
}
