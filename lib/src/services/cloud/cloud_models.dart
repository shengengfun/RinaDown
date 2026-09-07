// FluxCloud 账户/设备相关数据模型 —— 字段严格对照契约 v1（服务端 camelCase 直传JSON，
// 本文件只做「JSON → 强类型 Dart 对象」的薄封装，不含任何业务逻辑）。
//
// Entitlements 按契约建议保留原始 json（套餐能力字段由服务端自由演进，客户端
// 只对已知字段提供便捷读取，避免每加一个套餐字段就要跟着改模型）。

/// 用户状态，对应服务端 UserDto.status（"active"|"disabled"|"pending"）。
/// pending = 已注册但邮箱验证码尚未验证（两阶段注册的中间态）。
enum CloudUserStatus {
  active,
  disabled,
  pending;

  static CloudUserStatus fromWire(String? value) => switch (value) {
    'disabled' => CloudUserStatus.disabled,
    'pending' => CloudUserStatus.pending,
    _ => CloudUserStatus.active,
  };

  String get wireValue => switch (this) {
    CloudUserStatus.active => 'active',
    CloudUserStatus.disabled => 'disabled',
    CloudUserStatus.pending => 'pending',
  };
}

/// 云账户用户信息（对应服务端 UserDto）。
class CloudUser {
  final String id;
  final String email;
  final String nickname;
  final String plan;
  final CloudUserStatus status;
  final String createdAt;
  final String? lastLoginAt;

  /// 唯一数字身份（v1.2 新增，类 QQ 号）：激活时分配，pending 用户为 null。
  final int? originId;

  /// 是否已用掉自助修改 Origin ID 的唯一一次机会（v1.3 新增，见契约
  /// PUT /me/origin-id）；缺省 false（老快照/未激活用户）。
  final bool originIdChanged;

  /// 当前套餐下的会员编号（v1.4 新增）：仅当当前套餐 badgeNumbered=true 且
  /// 已分配时非空；切换套餐后变回 null（服务端行为，客户端不缓存推导）。
  final int? membershipOrdinal;

  const CloudUser({
    required this.id,
    required this.email,
    required this.nickname,
    required this.plan,
    required this.status,
    required this.createdAt,
    this.lastLoginAt,
    this.originId,
    this.originIdChanged = false,
    this.membershipOrdinal,
  });

  factory CloudUser.fromJson(Map<String, dynamic> json) => CloudUser(
    id: json['id'] as String,
    email: json['email'] as String,
    nickname: (json['nickname'] as String?) ?? '',
    plan: (json['plan'] as String?) ?? '',
    status: CloudUserStatus.fromWire(json['status'] as String?),
    createdAt: (json['createdAt'] as String?) ?? '',
    lastLoginAt: json['lastLoginAt'] as String?,
    originId: (json['originId'] as num?)?.toInt(),
    originIdChanged: (json['originIdChanged'] as bool?) ?? false,
    membershipOrdinal: (json['membershipOrdinal'] as num?)?.toInt(),
  );

  Map<String, dynamic> toJson() => {
    'id': id,
    'email': email,
    'nickname': nickname,
    'plan': plan,
    'status': status.wireValue,
    'createdAt': createdAt,
    'lastLoginAt': lastLoginAt,
    'originId': originId,
    'originIdChanged': originIdChanged,
    'membershipOrdinal': membershipOrdinal,
  };
}

/// 套餐能力集：保留服务端原始 json（见 server/crates/server/src/entitlement.rs 的
/// 前向兼容设计），仅对当前已知字段提供便捷读取，未知字段不丢失、不报错。
class Entitlements {
  final Map<String, dynamic> raw;

  const Entitlements(this.raw);

  factory Entitlements.fromJson(Map<String, dynamic>? json) =>
      Entitlements(json ?? const {});

  /// 同时保有登录会话/同步的设备数上限（同服务端 entitlement.rs 语义）。
  int get maxSyncDevices => (raw['maxSyncDevices'] as num?)?.toInt() ?? 0;

  /// 当前套餐是否允许自助修改一次 Origin ID（v1.3 新增）。
  bool get originIdEdit => (raw['originIdEdit'] as bool?) ?? false;

  Map<String, dynamic> toJson() => raw;
}

/// GET /me 响应：用户信息 + 套餐能力快照 + 差价升级抵扣基数。
class CloudProfile {
  final CloudUser user;
  final Entitlements entitlements;

  /// 当前套餐展示快照；即使套餐已下架也由服务端返回。
  final CloudPlan? currentPlan;

  /// 当前套餐的等效已付额（分）：购买更高档套餐时服务端按此抵扣；
  /// 免费/后台授予用户为 0。
  final int purchaseCreditMinor;

  const CloudProfile({
    required this.user,
    required this.entitlements,
    this.currentPlan,
    this.purchaseCreditMinor = 0,
  });

  factory CloudProfile.fromJson(Map<String, dynamic> json) => CloudProfile(
    user: CloudUser.fromJson(json),
    entitlements: Entitlements.fromJson(
      json['entitlements'] as Map<String, dynamic>?,
    ),
    currentPlan: json['currentPlan'] is Map<String, dynamic>
        ? CloudPlan.fromJson(json['currentPlan'] as Map<String, dynamic>)
        : null,
    purchaseCreditMinor: (json['purchaseCreditMinor'] as num?)?.toInt() ?? 0,
  );
}

/// GET /me/origin-id/check 响应：指定 Origin ID 是否可用；不可用时 [reason]
/// 为 "invalid"（格式不合法，如 <10000）或 "taken"（已被占用）。
class OriginIdCheckResult {
  final bool available;
  final String? reason;

  const OriginIdCheckResult({required this.available, this.reason});

  factory OriginIdCheckResult.fromJson(Map<String, dynamic> json) =>
      OriginIdCheckResult(
        available: json['available'] as bool? ?? false,
        reason: json['reason'] as String?,
      );
}

/// 受信任设备（对应服务端 DeviceDto）。
class CloudDevice {
  /// 服务端 devices 表行 id（PATCH/DELETE /devices/{id} 用这个）。
  final String id;

  /// 客户端持久设备标识（同 [DeviceIdentity.deviceId]，用于判断"是否当前设备"）。
  final String deviceId;
  final String name;
  final String? platform;
  final String createdAt;
  final String lastSeenAt;

  /// 最近登录 IP（服务端按 X-Forwarded-For 首项 → X-Real-IP → 对端地址记录，
  /// v1.1 新增，可空——旧设备行/未记录到时为 null）。
  final String? lastIp;

  /// 该设备最近一次发令牌请求携带的客户端版本号（v1.1 新增，可空）。
  final String? appVersion;

  /// 该设备当前是否在线（服务端按 SSE presence 连接实时判定，v1.2 多设备协同新增）。
  final bool isOnline;

  /// 是否为当前请求设备（服务端按请求头 deviceId 比对，v1.2 新增）。
  final bool isCurrent;

  const CloudDevice({
    required this.id,
    required this.deviceId,
    required this.name,
    this.platform,
    required this.createdAt,
    required this.lastSeenAt,
    this.lastIp,
    this.appVersion,
    this.isOnline = false,
    this.isCurrent = false,
  });

  factory CloudDevice.fromJson(Map<String, dynamic> json) => CloudDevice(
    id: json['id'] as String,
    deviceId: json['deviceId'] as String,
    name: (json['name'] as String?) ?? '',
    platform: json['platform'] as String?,
    createdAt: (json['createdAt'] as String?) ?? '',
    lastSeenAt: (json['lastSeenAt'] as String?) ?? '',
    lastIp: json['lastIp'] as String?,
    appVersion: json['appVersion'] as String?,
    isOnline: json['isOnline'] as bool? ?? false,
    isCurrent: json['isCurrent'] as bool? ?? false,
  );
}

/// 登录/注册验证/验证码登录 成功后的统一响应（AuthResponse）。
class AuthResponse {
  final String accessToken;
  final String refreshToken;
  final int expiresIn;
  final CloudUser user;
  final Entitlements entitlements;
  final CloudPlan? currentPlan;
  final CloudDevice device;

  const AuthResponse({
    required this.accessToken,
    required this.refreshToken,
    required this.expiresIn,
    required this.user,
    required this.entitlements,
    this.currentPlan,
    required this.device,
  });

  factory AuthResponse.fromJson(Map<String, dynamic> json) => AuthResponse(
    accessToken: json['accessToken'] as String,
    refreshToken: json['refreshToken'] as String,
    expiresIn: (json['expiresIn'] as num?)?.toInt() ?? 0,
    user: CloudUser.fromJson(json['user'] as Map<String, dynamic>),
    entitlements: Entitlements.fromJson(
      json['entitlements'] as Map<String, dynamic>?,
    ),
    currentPlan: json['currentPlan'] is Map<String, dynamic>
        ? CloudPlan.fromJson(json['currentPlan'] as Map<String, dynamic>)
        : null,
    device: CloudDevice.fromJson(json['device'] as Map<String, dynamic>),
  );
}

/// POST /auth/login 的 tagged 响应：设备已受信任则直接下发令牌（[LoginOk]）；
/// 新设备要求邮箱验证码，并通过响应字段预告验证后是否会替换最久未使用设备。
sealed class LoginResult {
  const LoginResult();
}

class LoginOk extends LoginResult {
  final AuthResponse auth;
  const LoginOk(this.auth);
}

class LoginDeviceVerificationRequired extends LoginResult {
  final int ttlSeconds;
  final bool willReplaceDevices;

  const LoginDeviceVerificationRequired(
    this.ttlSeconds, {
    this.willReplaceDevices = false,
  });
}

/// 服务端错误统一形态 `{code, message}`（见 error.rs），附带 HTTP 状态码方便
/// 调用方按状态/code 分支处理（如 409 registration_incomplete）。
class CloudApiException implements Exception {
  final String code;
  final String message;
  final int status;

  const CloudApiException({
    required this.code,
    required this.message,
    required this.status,
  });

  @override
  String toString() => 'CloudApiException($status $code: $message)';
}

/// 套餐活动档位（对应服务端 campaign.stages[]）：早鸟/首发/原价等阶梯定价，
/// [quota] 为该档位限量，null 表示不限量（通常是最后的"原价"档）。
class CloudPlanCampaignStage {
  final String label;
  final int priceMinor;
  final int? quota;

  const CloudPlanCampaignStage({
    required this.label,
    required this.priceMinor,
    this.quota,
  });

  factory CloudPlanCampaignStage.fromJson(Map<String, dynamic> json) =>
      CloudPlanCampaignStage(
        label: (json['label'] as String?) ?? '',
        priceMinor: (json['priceMinor'] as num?)?.toInt() ?? 0,
        quota: (json['quota'] as num?)?.toInt(),
      );

  /// 序列化回 wire 形态（供套餐目录本地快照落盘，与 [fromJson] 互逆）。
  Map<String, dynamic> toJson() => {
    'label': label,
    'priceMinor': priceMinor,
    'quota': quota,
  };
}

/// 套餐限时活动（仅活动 active 时随 catalog 下发）：阶梯限量定价 + 当前生效价快照。
class CloudPlanCampaign {
  final String name;
  final String? endAt;
  final List<CloudPlanCampaignStage> stages;
  final int soldTotal;
  final List<int> stageSold;
  final int currentStageIndex;
  final int effectivePriceMinor;

  const CloudPlanCampaign({
    required this.name,
    this.endAt,
    required this.stages,
    required this.soldTotal,
    required this.stageSold,
    required this.currentStageIndex,
    required this.effectivePriceMinor,
  });

  factory CloudPlanCampaign.fromJson(Map<String, dynamic> json) {
    final stages = (json['stages'] as List<dynamic>? ?? const [])
        .map((e) => CloudPlanCampaignStage.fromJson(e as Map<String, dynamic>))
        .toList();
    final stageSold = (json['stageSold'] as List<dynamic>? ?? const [])
        .map((e) => (e as num).toInt())
        .toList();
    return CloudPlanCampaign(
      name: (json['name'] as String?) ?? '',
      endAt: json['endAt'] as String?,
      stages: stages,
      soldTotal: (json['soldTotal'] as num?)?.toInt() ?? 0,
      stageSold: stageSold,
      currentStageIndex: (json['currentStageIndex'] as num?)?.toInt() ?? 0,
      effectivePriceMinor: (json['effectivePriceMinor'] as num?)?.toInt() ?? 0,
    );
  }

  /// 序列化回 wire 形态（供套餐目录本地快照落盘，与 [fromJson] 互逆）。
  Map<String, dynamic> toJson() => {
    'name': name,
    'endAt': endAt,
    'stages': [for (final s in stages) s.toJson()],
    'soldTotal': soldTotal,
    'stageSold': stageSold,
    'currentStageIndex': currentStageIndex,
    'effectivePriceMinor': effectivePriceMinor,
  };

  /// 当前生效档位；[currentStageIndex] 越界（服务端数据异常）兜底 null，UI 需容错。
  CloudPlanCampaignStage? get currentStage =>
      currentStageIndex >= 0 && currentStageIndex < stages.length
      ? stages[currentStageIndex]
      : null;

  /// 当前档位已售；[currentStageIndex] 越界兜底 0。
  int get currentStageSold =>
      currentStageIndex >= 0 && currentStageIndex < stageSold.length
      ? stageSold[currentStageIndex]
      : 0;
}

/// 公开套餐目录（GET /plans/catalog 响应元素，见契约 v1 CatalogPlanDto）。
/// 含上架套餐，以及下架但仍在官网展示的套餐（[purchasable] 为 false）。
class CloudPlan {
  final String code;
  final String name;
  final String description;
  final String? badge;
  final String icon;
  final String color;

  /// 徽标视觉样式（v1.4 新增，服务端 admin_plans.rs::BADGE_STYLES 白名单）：
  /// outline | solid | medal | ribbon。缺省 'outline'（兼容旧快照/未知值）。
  final String badgeStyle;

  /// 徽标专用强调色（v1.4 新增，独立于套餐整体识别色 [color]）。缺省空串时
  /// 渲染方应回退到 [color] 或主题 accent。
  final String badgeColor;

  /// 徽标是否追加会员编号（v1.4 新增，配合 [CloudUser.membershipOrdinal]）。
  final bool badgeNumbered;

  /// 会员编号补零位数（v1.4 新增，1-6，缺省 4）。
  final int badgeNumberDigits;

  final int priceMinor;
  final String currency;
  final List<String> highlights;

  /// 套餐能力集原始 json（同 [Entitlements] 的前向兼容设计，字段由服务端自由演进）。
  final Map<String, dynamic> entitlementsRaw;
  final int sort;
  final CloudPlanCampaign? campaign;

  /// 是否可在线购买。旧快照缺省视为 true。
  final bool purchasable;

  const CloudPlan({
    required this.code,
    required this.name,
    required this.description,
    this.badge,
    required this.icon,
    required this.color,
    this.badgeStyle = 'outline',
    this.badgeColor = '',
    this.badgeNumbered = false,
    this.badgeNumberDigits = 4,
    required this.priceMinor,
    required this.currency,
    required this.highlights,
    required this.entitlementsRaw,
    required this.sort,
    this.campaign,
    this.purchasable = true,
  });

  factory CloudPlan.fromJson(Map<String, dynamic> json) => CloudPlan(
    code: json['code'] as String,
    name: (json['name'] as String?) ?? '',
    description: (json['description'] as String?) ?? '',
    badge: json['badge'] as String?,
    icon: (json['icon'] as String?) ?? '',
    color: (json['color'] as String?) ?? '',
    badgeStyle: (json['badgeStyle'] as String?) ?? 'outline',
    badgeColor: (json['badgeColor'] as String?) ?? '',
    badgeNumbered: (json['badgeNumbered'] as bool?) ?? false,
    badgeNumberDigits: (json['badgeNumberDigits'] as num?)?.toInt() ?? 4,
    priceMinor: (json['priceMinor'] as num?)?.toInt() ?? 0,
    currency: (json['currency'] as String?) ?? 'CNY',
    highlights: (json['highlights'] as List<dynamic>? ?? const [])
        .map((e) => e as String)
        .toList(),
    entitlementsRaw:
        (json['entitlements'] as Map<String, dynamic>?) ?? const {},
    sort: (json['sort'] as num?)?.toInt() ?? 0,
    purchasable: json['purchasable'] as bool? ?? true,
    campaign: json['campaign'] is Map<String, dynamic>
        ? CloudPlanCampaign.fromJson(json['campaign'] as Map<String, dynamic>)
        : null,
  );

  /// 序列化回 wire 形态（供套餐目录本地快照落盘，与 [fromJson] 互逆）。
  Map<String, dynamic> toJson() => {
    'code': code,
    'name': name,
    'description': description,
    'badge': badge,
    'icon': icon,
    'color': color,
    'badgeStyle': badgeStyle,
    'badgeColor': badgeColor,
    'badgeNumbered': badgeNumbered,
    'badgeNumberDigits': badgeNumberDigits,
    'priceMinor': priceMinor,
    'currency': currency,
    'highlights': highlights,
    'entitlements': entitlementsRaw,
    'sort': sort,
    'purchasable': purchasable,
    if (campaign != null) 'campaign': campaign!.toJson(),
  };

  /// 实际成交价：活动生效价优先，否则套餐基础价（同契约「购买价」定义）。
  int get effectivePriceMinor => campaign?.effectivePriceMinor ?? priceMinor;
}

/// 购买订单（POST/GET /orders 响应，见契约 v1 OrderDto）。[status] 取值
/// pending | paid | failed | expired，见下方 isPending 等便捷 getter。
class CloudOrder {
  final String orderNo;
  final String planCode;
  final String planName;
  final String status;
  final int amountMinor;
  final int listPriceMinor;

  /// 差价升级抵扣额（分，0 = 全款单）；等效全款 = creditMinor + amountMinor。
  final int creditMinor;
  final String? upgradeFromPlan;
  final String currency;
  final String? campaignName;
  final String? stageLabel;

  /// 下单时填写的推荐码（无码或推荐系统未生效时为 null）。
  final String? referralCode;

  /// 推荐码带来的立减额（分，0 = 无立减），已内含在 [amountMinor] 中。
  final int referralDiscountMinor;

  /// 微信 Native 收款二维码内容；failed 时可能为 null。
  final String? codeUrl;
  final String createdAt;
  final String? paidAt;
  final String expiresAt;

  const CloudOrder({
    required this.orderNo,
    required this.planCode,
    required this.planName,
    required this.status,
    required this.amountMinor,
    required this.listPriceMinor,
    this.creditMinor = 0,
    this.upgradeFromPlan,
    required this.currency,
    this.campaignName,
    this.stageLabel,
    this.referralCode,
    this.referralDiscountMinor = 0,
    this.codeUrl,
    required this.createdAt,
    this.paidAt,
    required this.expiresAt,
  });

  factory CloudOrder.fromJson(Map<String, dynamic> json) => CloudOrder(
    orderNo: json['orderNo'] as String,
    planCode: json['planCode'] as String,
    planName: (json['planName'] as String?) ?? '',
    status: (json['status'] as String?) ?? 'pending',
    amountMinor: (json['amountMinor'] as num?)?.toInt() ?? 0,
    listPriceMinor: (json['listPriceMinor'] as num?)?.toInt() ?? 0,
    creditMinor: (json['creditMinor'] as num?)?.toInt() ?? 0,
    upgradeFromPlan: json['upgradeFromPlan'] as String?,
    currency: (json['currency'] as String?) ?? 'CNY',
    campaignName: json['campaignName'] as String?,
    stageLabel: json['stageLabel'] as String?,
    referralCode: json['referralCode'] as String?,
    referralDiscountMinor:
        (json['referralDiscountMinor'] as num?)?.toInt() ?? 0,
    codeUrl: json['codeUrl'] as String?,
    createdAt: (json['createdAt'] as String?) ?? '',
    paidAt: json['paidAt'] as String?,
    expiresAt: (json['expiresAt'] as String?) ?? '',
  );

  bool get isPending => status == 'pending';
  bool get isPaid => status == 'paid';
  bool get isFailed => status == 'failed';
  bool get isExpired => status == 'expired';
}

/// 推介有奖规则档位（GET /referral/summary 响应 rules[] 元素）：遍历 rules
/// 只含 enabled 且 price_minor>0 的套餐，按解析后的生效值下发。
class CloudReferralRule {
  final String planCode;
  final String planName;
  final int priceMinor;
  final int discountMinor;
  final int rewardPercent;

  const CloudReferralRule({
    required this.planCode,
    required this.planName,
    required this.priceMinor,
    required this.discountMinor,
    required this.rewardPercent,
  });

  factory CloudReferralRule.fromJson(Map<String, dynamic> json) =>
      CloudReferralRule(
        planCode: json['planCode'] as String,
        planName: (json['planName'] as String?) ?? '',
        priceMinor: (json['priceMinor'] as num?)?.toInt() ?? 0,
        discountMinor: (json['discountMinor'] as num?)?.toInt() ?? 0,
        rewardPercent: (json['rewardPercent'] as num?)?.toInt() ?? 0,
      );

  /// 序列化回 wire 形态（供推介页说明内容本地缓存落盘，stale-while-revalidate
  /// 场景下与 [fromJson] 互逆）。
  Map<String, dynamic> toJson() => {
    'planCode': planCode,
    'planName': planName,
    'priceMinor': priceMinor,
    'discountMinor': discountMinor,
    'rewardPercent': rewardPercent,
  };
}

/// GET /referral/summary 响应：推介有奖总览——收益统计 + 说明文案 + 规则表。
/// [enabled]=false 时功能未开启，其余字段仍照常返回。多码化后不再随 summary
/// 下发单一推荐码，见 [CloudReferralCode] / `GET /referral/codes`。
class CloudReferralSummary {
  final bool enabled;

  /// 后台可编辑的说明文案（多行，保留换行）；空串时客户端回退到 [rules] 自动渲染。
  final String description;

  /// 当前账号是否参与返现（管理员可为内部账号关闭；关闭后码仍可用于买家立减）。
  final bool rewardEnabled;
  final String contact;
  final int invitedCount;
  final int pendingRewardMinor;
  final int paidRewardMinor;
  final int totalRewardMinor;
  final List<CloudReferralRule> rules;

  const CloudReferralSummary({
    required this.enabled,
    required this.description,
    required this.rewardEnabled,
    required this.contact,
    required this.invitedCount,
    required this.pendingRewardMinor,
    required this.paidRewardMinor,
    required this.totalRewardMinor,
    required this.rules,
  });

  factory CloudReferralSummary.fromJson(Map<String, dynamic> json) =>
      CloudReferralSummary(
        enabled: json['enabled'] as bool? ?? false,
        description: (json['description'] as String?) ?? '',
        rewardEnabled: json['rewardEnabled'] as bool? ?? true,
        contact: (json['contact'] as String?) ?? '',
        invitedCount: (json['invitedCount'] as num?)?.toInt() ?? 0,
        pendingRewardMinor: (json['pendingRewardMinor'] as num?)?.toInt() ?? 0,
        paidRewardMinor: (json['paidRewardMinor'] as num?)?.toInt() ?? 0,
        totalRewardMinor: (json['totalRewardMinor'] as num?)?.toInt() ?? 0,
        rules: (json['rules'] as List<dynamic>? ?? const [])
            .map((e) => CloudReferralRule.fromJson(e as Map<String, dynamic>))
            .toList(),
      );

  /// 序列化回 wire 形态（供推介页说明内容本地缓存落盘：先展示缓存、后台静默
  /// 刷新，与 [fromJson] 互逆）。
  Map<String, dynamic> toJson() => {
    'enabled': enabled,
    'description': description,
    'rewardEnabled': rewardEnabled,
    'contact': contact,
    'invitedCount': invitedCount,
    'pendingRewardMinor': pendingRewardMinor,
    'paidRewardMinor': paidRewardMinor,
    'totalRewardMinor': totalRewardMinor,
    'rules': [for (final r in rules) r.toJson()],
  };
}

/// GET/POST /referral/codes 响应单条：一个可自定义或随机生成的推荐码及其统计。
/// [paidOrderCount] 为该码文本快照命中的本人 status=paid 订单数，[rewardMinor]
/// 为关联订单的返利台账合计（pending+paid，排除 revoked）。
class CloudReferralCode {
  final String id;
  final String code;
  final int paidOrderCount;
  final int rewardMinor;
  final String createdAt;

  const CloudReferralCode({
    required this.id,
    required this.code,
    required this.paidOrderCount,
    required this.rewardMinor,
    required this.createdAt,
  });

  factory CloudReferralCode.fromJson(Map<String, dynamic> json) =>
      CloudReferralCode(
        id: json['id'] as String,
        code: (json['code'] as String?) ?? '',
        paidOrderCount: (json['paidOrderCount'] as num?)?.toInt() ?? 0,
        rewardMinor: (json['rewardMinor'] as num?)?.toInt() ?? 0,
        createdAt: (json['createdAt'] as String?) ?? '',
      );
}

/// GET /referral/codes 响应：总条数 + 当页推荐码列表。
class CloudReferralCodesResult {
  final int total;
  final List<CloudReferralCode> items;

  const CloudReferralCodesResult({required this.total, required this.items});

  factory CloudReferralCodesResult.fromJson(Map<String, dynamic> json) =>
      CloudReferralCodesResult(
        total: (json['total'] as num?)?.toInt() ?? 0,
        items: (json['items'] as List<dynamic>? ?? const [])
            .map((e) => CloudReferralCode.fromJson(e as Map<String, dynamic>))
            .toList(),
      );
}

/// GET /referral/records 响应单条：一笔已归因给我的订单产生的返利台账行。
/// [status] 取值 pending（待兑付）| paid（已兑付）| revoked（已撤销）。
class CloudReferralRecord {
  final String id;

  /// 买家脱敏展示名（服务端已脱敏：优先昵称，否则邮箱首字符+***+@域名）。
  final String buyerLabel;
  final int orderAmountMinor;
  final int rewardMinor;
  final int rewardPercent;
  final String status;
  final String createdAt;
  final String? paidAt;

  /// 该笔订单下单时使用的推荐码文本快照（无码时为 null）。
  final String? referralCode;

  const CloudReferralRecord({
    required this.id,
    required this.buyerLabel,
    required this.orderAmountMinor,
    required this.rewardMinor,
    required this.rewardPercent,
    required this.status,
    required this.createdAt,
    this.paidAt,
    this.referralCode,
  });

  factory CloudReferralRecord.fromJson(Map<String, dynamic> json) =>
      CloudReferralRecord(
        id: json['id'] as String,
        buyerLabel: (json['buyerLabel'] as String?) ?? '',
        orderAmountMinor: (json['orderAmountMinor'] as num?)?.toInt() ?? 0,
        rewardMinor: (json['rewardMinor'] as num?)?.toInt() ?? 0,
        rewardPercent: (json['rewardPercent'] as num?)?.toInt() ?? 0,
        status: (json['status'] as String?) ?? 'pending',
        createdAt: (json['createdAt'] as String?) ?? '',
        paidAt: json['paidAt'] as String?,
        referralCode: json['referralCode'] as String?,
      );

  bool get isPending => status == 'pending';
  bool get isPaid => status == 'paid';
  bool get isRevoked => status == 'revoked';
}

/// GET /referral/records 响应：总条数 + 当页收益记录列表。
class CloudReferralRecordsResult {
  final int total;
  final List<CloudReferralRecord> items;

  const CloudReferralRecordsResult({required this.total, required this.items});

  factory CloudReferralRecordsResult.fromJson(Map<String, dynamic> json) =>
      CloudReferralRecordsResult(
        total: (json['total'] as num?)?.toInt() ?? 0,
        items: (json['items'] as List<dynamic>? ?? const [])
            .map((e) => CloudReferralRecord.fromJson(e as Map<String, dynamic>))
            .toList(),
      );
}

/// GET /referral/validate 响应：下单前预校验推荐码是否可用。[reason] 仅在
/// [valid]=false 时非空，取值 feature_disabled | plan_excluded | not_found |
/// self_use | already_used | no_discount。
class CloudReferralValidateResult {
  final bool valid;
  final int discountMinor;
  final String? reason;

  const CloudReferralValidateResult({
    required this.valid,
    required this.discountMinor,
    this.reason,
  });

  factory CloudReferralValidateResult.fromJson(Map<String, dynamic> json) =>
      CloudReferralValidateResult(
        valid: json['valid'] as bool? ?? false,
        discountMinor: (json['discountMinor'] as num?)?.toInt() ?? 0,
        reason: json['reason'] as String?,
      );
}

/// 配置同步单条目（对应服务端 GET /sync/items 响应的 items[]，见契约 v1 数据模型）。
/// [value] 为任意 JSON 值（bool/number/string/…），墓碑条目（[deleted]=true）时为 null。
class SyncItem {
  final String key;
  final dynamic value;
  final bool deleted;
  final int version;
  final String deviceId;
  final String? deviceName;
  final String updatedAt;

  const SyncItem({
    required this.key,
    required this.value,
    required this.deleted,
    required this.version,
    required this.deviceId,
    this.deviceName,
    required this.updatedAt,
  });

  factory SyncItem.fromJson(Map<String, dynamic> json) => SyncItem(
    key: json['key'] as String,
    value: json['value'],
    deleted: (json['deleted'] as bool?) ?? false,
    version: (json['version'] as num?)?.toInt() ?? 0,
    deviceId: (json['deviceId'] as String?) ?? '',
    deviceName: json['deviceName'] as String?,
    updatedAt: (json['updatedAt'] as String?) ?? '',
  );
}

/// GET /sync/items 响应：当前修订号 + 是否强制重同步 + 变更条目列表。
class SyncPullResult {
  final int revision;
  final bool resync;
  final List<SyncItem> items;

  const SyncPullResult({
    required this.revision,
    required this.resync,
    required this.items,
  });

  factory SyncPullResult.fromJson(Map<String, dynamic> json) => SyncPullResult(
    revision: (json['revision'] as num?)?.toInt() ?? 0,
    resync: (json['resync'] as bool?) ?? false,
    items: (json['items'] as List<dynamic>? ?? const [])
        .map((e) => SyncItem.fromJson(e as Map<String, dynamic>))
        .toList(),
  );
}

/// 跨设备任务状态（对应服务端 cross_device_tasks.status）。
enum RemoteTaskStatus {
  pending,
  accepted,
  downloading,
  paused,
  completed,
  failed,
  canceled;

  static RemoteTaskStatus fromWire(String s) => switch (s) {
    'accepted' => accepted,
    'downloading' => downloading,
    'paused' => paused,
    'completed' => completed,
    'failed' => failed,
    'canceled' => canceled,
    _ => pending,
  };

  String get wire => name;

  bool get isTerminal =>
      this == completed || this == failed || this == canceled;
}

/// 跨设备任务（对应服务端 RemoteTaskDto）。进度字段来自服务端内存快照，
/// 经 SSE task.progress 增量更新（见 RemoteTaskService），不落库。
class RemoteTask {
  final String id;
  final String fromDevice;
  final String toDevice;
  final String url;
  final String? saveDir;
  final String fileName;
  final RemoteTaskStatus status;
  final int? totalBytes;
  final int downloadedBytes;
  final int speed;
  final double progress;
  final String? error;
  final String createdAt;
  final String updatedAt;

  const RemoteTask({
    required this.id,
    required this.fromDevice,
    required this.toDevice,
    required this.url,
    this.saveDir,
    this.fileName = '',
    this.status = RemoteTaskStatus.pending,
    this.totalBytes,
    this.downloadedBytes = 0,
    this.speed = 0,
    this.progress = 0,
    this.error,
    this.createdAt = '',
    this.updatedAt = '',
  });

  factory RemoteTask.fromJson(Map<String, dynamic> json) => RemoteTask(
    id: json['id'] as String,
    fromDevice: (json['fromDevice'] as String?) ?? '',
    toDevice: (json['toDevice'] as String?) ?? '',
    url: (json['url'] as String?) ?? '',
    saveDir: json['saveDir'] as String?,
    fileName: (json['fileName'] as String?) ?? '',
    status: RemoteTaskStatus.fromWire((json['status'] as String?) ?? 'pending'),
    totalBytes: (json['totalBytes'] as num?)?.toInt(),
    downloadedBytes: (json['downloadedBytes'] as num?)?.toInt() ?? 0,
    speed: (json['speed'] as num?)?.toInt() ?? 0,
    progress: (json['progress'] as num?)?.toDouble() ?? 0,
    error: json['error'] as String?,
    createdAt: (json['createdAt'] as String?) ?? '',
    updatedAt: (json['updatedAt'] as String?) ?? '',
  );

  /// SSE 增量更新：只覆盖传入的非空字段，其余保留（进度回流高频路径，避免重建全对象）。
  RemoteTask copyWith({
    RemoteTaskStatus? status,
    int? totalBytes,
    int? downloadedBytes,
    int? speed,
    double? progress,
    String? fileName,
    String? error,
    String? updatedAt,
  }) => RemoteTask(
    id: id,
    fromDevice: fromDevice,
    toDevice: toDevice,
    url: url,
    saveDir: saveDir,
    fileName: fileName ?? this.fileName,
    status: status ?? this.status,
    totalBytes: totalBytes ?? this.totalBytes,
    downloadedBytes: downloadedBytes ?? this.downloadedBytes,
    speed: speed ?? this.speed,
    progress: progress ?? this.progress,
    error: error ?? this.error,
    createdAt: createdAt,
    updatedAt: updatedAt ?? this.updatedAt,
  );
}

/// 执行端批量上报进度的单条载荷（POST /tasks/progress 的 items[]）。
class ProgressReport {
  final String taskId;
  final int downloadedBytes;
  final int speed;
  final double progress;

  const ProgressReport({
    required this.taskId,
    required this.downloadedBytes,
    required this.speed,
    required this.progress,
  });

  Map<String, dynamic> toJson() => {
    'taskId': taskId,
    'downloadedBytes': downloadedBytes,
    'speed': speed,
    'progress': progress,
  };
}

/// CDN 聚合下载云端解析节点（GET /cdn/config 响应 resolvers[]）。
class CdnResolverEntry {
  final String url;
  final bool ecs;

  const CdnResolverEntry({required this.url, required this.ecs});

  factory CdnResolverEntry.fromJson(Map<String, dynamic> json) =>
      CdnResolverEntry(
        url: json['url'] as String? ?? '',
        ecs: json['ecs'] as bool? ?? false,
      );
}

/// CDN 聚合下载云端 ECS 子网先验（GET /cdn/config 响应 ecs_subnets[]，P2 新增）：
/// resolver 发起 EDNS Client Subnet 查询时按 [subnet] 附带的地域先验，
/// [region]/[isp] 仅供后台管理展示，客户端引擎只消费 [subnet]。
class CdnEcsSubnetEntry {
  final String region;
  final String isp;
  final String subnet;

  const CdnEcsSubnetEntry({
    required this.region,
    required this.isp,
    required this.subnet,
  });

  factory CdnEcsSubnetEntry.fromJson(Map<String, dynamic> json) =>
      CdnEcsSubnetEntry(
        region: json['region'] as String? ?? '',
        isp: json['isp'] as String? ?? '',
        subnet: json['subnet'] as String? ?? '',
      );
}

/// GET /cdn/config 响应（P1 §四 + P2 §五契约）：CDN 多节点聚合下载云端配置快照。
/// 字段名为服务端约定的 snake_case（直接对应 FluxDown 引擎 config 表键，
/// 与本文件其余模型的 camelCase 约定不同——见契约「客户端行为」节）。
/// 云端只下发先验，不做套餐门控：是否启用聚合、并发节点数上限均为客户端本地设置。
/// [policy] 暂不解析（引擎侧聚合超时预算等仍走本地默认值）。
class CdnConfig {
  final int revision;
  final List<CdnResolverEntry> resolvers;
  final List<CdnEcsSubnetEntry> ecsSubnets;

  const CdnConfig({
    required this.revision,
    required this.resolvers,
    required this.ecsSubnets,
  });

  factory CdnConfig.fromJson(Map<String, dynamic> json) {
    final resolversJson = json['resolvers'] as List<dynamic>? ?? const [];
    final ecsSubnetsJson = json['ecs_subnets'] as List<dynamic>? ?? const [];
    return CdnConfig(
      revision: (json['revision'] as num?)?.toInt() ?? 0,
      resolvers: resolversJson
          .map((e) => CdnResolverEntry.fromJson(e as Map<String, dynamic>))
          .toList(),
      ecsSubnets: ecsSubnetsJson
          .map((e) => CdnEcsSubnetEntry.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }
}

/// [CloudClient.fetchCdnConfig] 结果。304 命中时 [notModified] 为 true，
/// [etag]/[config] 均为 null——调用方应保留本地已持久化的旧值不动。
class CdnConfigResult {
  final String? etag;
  final CdnConfig? config;
  final bool notModified;

  const CdnConfigResult({this.etag, this.config}) : notModified = false;

  const CdnConfigResult.notModified()
    : etag = null,
      config = null,
      notModified = true;
}
