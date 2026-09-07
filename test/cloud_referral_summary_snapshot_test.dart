// 推介有奖说明本地缓存契约（settings_page _ReferralGateState 冷启动
// stale-while-revalidate）：CloudReferralSummary.toJson 必须与 fromJson
// 互逆，否则本地缓存恢复的说明文案/规则表会与上次网络拉取的不一致。

import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';

import 'package:flux_down/src/services/cloud/cloud_models.dart';

void main() {
  test('CloudReferralSummary 快照经 jsonEncode/Decode 往返后字段逐项一致', () {
    const wire = {
      'enabled': true,
      'description': '邀请好友，双方立减',
      'rewardEnabled': true,
      'contact': 'support@example.com',
      'invitedCount': 7,
      'pendingRewardMinor': 1200,
      'paidRewardMinor': 3400,
      'totalRewardMinor': 4600,
      'rules': [
        {
          'planCode': 'plus',
          'planName': '进阶版',
          'priceMinor': 9900,
          'discountMinor': 990,
          'rewardPercent': 10,
        },
        {
          'planCode': 'founder',
          'planName': '创始会员',
          'priceMinor': 19900,
          'discountMinor': 1990,
          'rewardPercent': 15,
        },
      ],
    };

    final restored = CloudReferralSummary.fromJson(
      jsonDecode(jsonEncode(CloudReferralSummary.fromJson(wire).toJson()))
          as Map<String, dynamic>,
    );

    expect(restored.enabled, isTrue);
    expect(restored.description, '邀请好友，双方立减');
    expect(restored.rewardEnabled, isTrue);
    expect(restored.contact, 'support@example.com');
    expect(restored.invitedCount, 7);
    expect(restored.pendingRewardMinor, 1200);
    expect(restored.paidRewardMinor, 3400);
    expect(restored.totalRewardMinor, 4600);
    expect(restored.rules, hasLength(2));
    expect(restored.rules[0].planCode, 'plus');
    expect(restored.rules[0].planName, '进阶版');
    expect(restored.rules[0].priceMinor, 9900);
    expect(restored.rules[0].discountMinor, 990);
    expect(restored.rules[0].rewardPercent, 10);
    expect(restored.rules[1].planCode, 'founder');
    expect(restored.rules[1].rewardPercent, 15);
  });

  test('禁用状态且无规则的快照往返后仍保持 disabled/空 rules', () {
    const wire = {'enabled': false, 'contact': ''};
    final restored = CloudReferralSummary.fromJson(
      jsonDecode(jsonEncode(CloudReferralSummary.fromJson(wire).toJson()))
          as Map<String, dynamic>,
    );
    expect(restored.enabled, isFalse);
    expect(restored.rules, isEmpty);
    expect(restored.description, '');
  });
}
