import 'dart:convert';

/// Webhook 端点模型（免费自托管 BYOE）。
///
/// **字段与 wire 契约一一对应**：整个列表以 JSON 数组存进引擎 config 表的
/// `webhook.endpoints` 键，Rust 侧 `fluxdown_engine::webhook::EndpointSpec`
/// 是同一份 schema（camelCase）。加字段必须两侧同步，否则静默丢配置。
class WebhookEndpoint {
  const WebhookEndpoint({
    required this.id,
    this.name = '',
    this.preset = kPresetCustom,
    this.url = '',
    this.enabled = true,
    this.events = const [],
    this.queueId = '',
    this.headers = const {},
    this.bodyTemplate = '',
    this.signSecret = '',
    this.allowHttp = false,
    this.useProxy = false,
  });

  /// 未知/自定义预设的 wire 名（引擎对未知值也降级到这里）。
  static const String kPresetCustom = 'custom';

  final String id;
  final String name;
  final String preset;
  final String url;
  final bool enabled;

  /// 订阅的事件 wire 名（`task.completed` 等）。空 = 不投递任何事件。
  final List<String> events;

  /// 队列过滤：空 = 全部队列。
  final String queueId;

  /// 自定义请求头（可覆盖 Content-Type，承载各服务 token）。
  final Map<String, String> headers;

  /// 自定义 body 模板；空 = 用预设默认模板。
  final String bodyTemplate;

  /// 非空则开启 HMAC-SHA256 签名。
  final String signSecret;

  /// 允许 http:// 明文（仅建议局域网设备）。
  final bool allowHttp;

  /// 经全局代理发送（默认直连——局域网端点走代理必失败）。
  final bool useProxy;

  WebhookEndpoint copyWith({
    String? name,
    String? preset,
    String? url,
    bool? enabled,
    List<String>? events,
    String? queueId,
    Map<String, String>? headers,
    String? bodyTemplate,
    String? signSecret,
    bool? allowHttp,
    bool? useProxy,
  }) {
    return WebhookEndpoint(
      id: id,
      name: name ?? this.name,
      preset: preset ?? this.preset,
      url: url ?? this.url,
      enabled: enabled ?? this.enabled,
      events: events ?? this.events,
      queueId: queueId ?? this.queueId,
      headers: headers ?? this.headers,
      bodyTemplate: bodyTemplate ?? this.bodyTemplate,
      signSecret: signSecret ?? this.signSecret,
      allowHttp: allowHttp ?? this.allowHttp,
      useProxy: useProxy ?? this.useProxy,
    );
  }

  Map<String, dynamic> toJson() => {
    'id': id,
    'name': name,
    'preset': preset,
    'url': url,
    'enabled': enabled,
    'events': events,
    'queueId': queueId,
    'headers': headers,
    'bodyTemplate': bodyTemplate,
    'signSecret': signSecret,
    'allowHttp': allowHttp,
    'useProxy': useProxy,
  };

  factory WebhookEndpoint.fromJson(Map<String, dynamic> json) {
    return WebhookEndpoint(
      id: json['id'] as String? ?? '',
      name: json['name'] as String? ?? '',
      preset: json['preset'] as String? ?? kPresetCustom,
      url: json['url'] as String? ?? '',
      // 缺字段视为启用——与引擎 `default_true` 对齐。
      enabled: json['enabled'] as bool? ?? true,
      events:
          (json['events'] as List<dynamic>?)?.map((e) => e.toString()).toList() ??
          const [],
      queueId: json['queueId'] as String? ?? '',
      headers:
          (json['headers'] as Map<dynamic, dynamic>?)?.map(
            (k, v) => MapEntry(k.toString(), v?.toString() ?? ''),
          ) ??
          const {},
      bodyTemplate: json['bodyTemplate'] as String? ?? '',
      signSecret: json['signSecret'] as String? ?? '',
      allowHttp: json['allowHttp'] as bool? ?? false,
      useProxy: json['useProxy'] as bool? ?? false,
    );
  }

  static String encodeList(List<WebhookEndpoint> list) =>
      jsonEncode(list.map((e) => e.toJson()).toList());

  /// 解析失败返回空列表——绝不让一份坏配置炸掉整个设置页。
  static List<WebhookEndpoint> decodeList(String json) {
    if (json.trim().isEmpty) return [];
    try {
      final list = jsonDecode(json) as List<dynamic>;
      return list
          .map((e) => WebhookEndpoint.fromJson(e as Map<String, dynamic>))
          .toList();
    } catch (_) {
      return [];
    }
  }
}

/// v1 事件集（wire 名即契约，与 `WebhookEventKind::wire()` 逐条对齐）。
class WebhookEvents {
  const WebhookEvents._();

  static const String taskCreated = 'task.created';
  static const String taskStarted = 'task.started';
  static const String taskCompleted = 'task.completed';
  static const String taskFailed = 'task.failed';
  static const String taskPaused = 'task.paused';
  static const String queueDrained = 'queue.drained';

  /// 顺序即 UI 芯片顺序。
  static const List<String> all = [
    taskCreated,
    taskStarted,
    taskCompleted,
    taskFailed,
    taskPaused,
    queueDrained,
  ];

  /// 新端点默认订阅：完成 + 失败覆盖 80% 场景，零调整即可用。
  static const List<String> defaults = [taskCompleted, taskFailed];
}
