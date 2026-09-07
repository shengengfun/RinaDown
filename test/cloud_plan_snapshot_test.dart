// 套餐目录本地快照契约（账户页 cloud_plans_catalog / web fluxdown.cloud.plansCatalog）：
// CloudPlan.toJson 必须与 fromJson 互逆，否则冷启动从快照恢复的徽标会与
// 上次网络拉取渲染的不一致（错样式/丢编号），违背「上次最终显示的 UI」承诺。

import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';

import 'package:flux_down/src/services/cloud/cloud_models.dart';

void main() {
  test('CloudPlan 快照经 jsonEncode/Decode 往返后徽标与价格字段逐项一致', () {
    const wire = {
      'code': 'founder',
      'name': '创始会员',
      'description': '限量买断',
      'badge': '创始会员',
      'icon': 'crown',
      'color': '#b8860b',
      'badgeStyle': 'medal',
      'badgeColor': '#d97706',
      'badgeNumbered': true,
      'badgeNumberDigits': 4,
      'priceMinor': 19900,
      'currency': 'CNY',
      'highlights': ['终身有效', '专属徽标'],
      'entitlements': {'maxSyncDevices': 10, 'originIdEdit': true},
      'sort': 5,
      'campaign': {
        'name': '首发',
        'endAt': '2026-09-01T00:00:00Z',
        'stages': [
          {'label': '早鸟', 'priceMinor': 14900, 'quota': 100},
          {'label': '原价', 'priceMinor': 19900, 'quota': null},
        ],
        'soldTotal': 42,
        'stageSold': [42, 0],
        'currentStageIndex': 0,
        'effectivePriceMinor': 14900,
      },
    };

    final restored = CloudPlan.fromJson(
      jsonDecode(jsonEncode(CloudPlan.fromJson(wire).toJson()))
          as Map<String, dynamic>,
    );

    expect(restored.code, 'founder');
    expect(restored.badge, '创始会员');
    expect(restored.badgeStyle, 'medal');
    expect(restored.badgeColor, '#d97706');
    expect(restored.badgeNumbered, isTrue);
    expect(restored.badgeNumberDigits, 4);
    expect(restored.icon, 'crown');
    expect(restored.color, '#b8860b');
    expect(restored.priceMinor, 19900);
    expect(restored.effectivePriceMinor, 14900);
    expect(restored.entitlementsRaw['maxSyncDevices'], 10);
    final campaign = restored.campaign!;
    expect(campaign.currentStage!.label, '早鸟');
    expect(campaign.currentStage!.quota, 100);
    expect(campaign.stages[1].quota, isNull);
    expect(campaign.stageSold, [42, 0]);
  });

  test('用户资料保留下架当前套餐的徽标展示快照', () {
    const currentPlan = {
      'code': 'legacy',
      'name': '典藏套餐',
      'description': '已下架但仍归属用户',
      'badge': '典藏会员',
      'icon': 'crown',
      'color': '#6d28d9',
      'badgeStyle': 'medal',
      'badgeColor': '#7c3aed',
      'badgeNumbered': true,
      'badgeNumberDigits': 5,
    };
    const profileWire = {
      'id': 'u1',
      'email': 'u1@example.com',
      'nickname': '用户',
      'plan': 'legacy',
      'status': 'active',
      'createdAt': '2026-08-16T00:00:00Z',
      'entitlements': <String, dynamic>{},
      'currentPlan': currentPlan,
    };

    final profile = CloudProfile.fromJson(profileWire);

    expect(profile.currentPlan?.code, profile.user.plan);
    expect(profile.currentPlan?.name, '典藏套餐');
    expect(profile.currentPlan?.badge, '典藏会员');
    expect(profile.currentPlan?.badgeStyle, 'medal');
    expect(profile.currentPlan?.badgeColor, '#7c3aed');
    expect(profile.currentPlan?.badgeNumbered, isTrue);
    expect(profile.currentPlan?.badgeNumberDigits, 5);
  });

  test('无 campaign / 无徽标的免费档快照往返后 badge 保持 null', () {
    const wire = {'code': 'free', 'name': '免费版'};
    final restored = CloudPlan.fromJson(
      jsonDecode(jsonEncode(CloudPlan.fromJson(wire).toJson()))
          as Map<String, dynamic>,
    );
    expect(restored.badge, isNull);
    expect(restored.campaign, isNull);
    expect(restored.badgeStyle, 'outline');
  });
}
