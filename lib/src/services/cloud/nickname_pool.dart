// 默认昵称词库 —— "盲盒兽名"：形容词 + 动物 的双语组合，主题取自下载圈熟悉的
// 速度 × 囤积梗（"满速的仓鼠"、"蹲种的浣熊"……），替代千人一面的默认昵称
// （原「FluxDown 用户」）。
//
// 中英文数组按下标严格对齐（同一下标语义相同），保证注册表单预填、验证码
// 登录自动注册等场景无论界面语言是中文还是英文都能生成语气一致的默认昵称。
// 本词库仅供客户端本地生成默认值使用，不改变昵称字段的服务端契约——
// wire 上昵称仍是任意字符串（服务端另有同款词库用于自身兜底生成，见
// local://fluxcloud-auth-contract.md 备注，两端各自实现互不依赖）。
//
// 纯函数、无副作用，便于单测：[seed] 缺省为空时用真随机，传入固定值可复现。

import 'dart:math';

class NicknamePool {
  NicknamePool._();

  /// 形容词词库（中）：速度 × 囤积主题，与 [adjectivesEn] 下标一一对应。
  static const List<String> adjectivesZh = [
    '满速的',
    '囤货的',
    '蹲种的',
    '疾风的',
    '通宵的',
    '爆仓的',
    '拆包的',
    '追更的',
    '破浪的',
    '闪电的',
    '挂机的',
    '起飞的',
  ];

  /// 形容词词库（英），与 [adjectivesZh] 下标一一对应。
  static const List<String> adjectivesEn = [
    'Full-Speed',
    'Hoarding',
    'Seeding',
    'Gale',
    'All-Night',
    'Overstocked',
    'Unboxing',
    'Binge',
    'Wavebreaker',
    'Lightning',
    'Idle',
    'Takeoff',
  ];

  /// 动物词库（中），与 [animalsEn] 下标一一对应。
  static const List<String> animalsZh = [
    '仓鼠',
    '浣熊',
    '企鹅',
    '隼',
    '树懒',
    '松鼠',
    '信天翁',
    '旗鱼',
    '雪貂',
    '狸猫',
    '蜂鸟',
    '水獭',
  ];

  /// 动物词库（英），与 [animalsZh] 下标一一对应。
  static const List<String> animalsEn = [
    'Hamster',
    'Raccoon',
    'Penguin',
    'Falcon',
    'Sloth',
    'Squirrel',
    'Albatross',
    'Sailfish',
    'Ferret',
    'Tanuki',
    'Hummingbird',
    'Otter',
  ];

  /// 生成一个「形容词 + 动物」默认昵称建议。
  ///
  /// [isZh] 为 true 时用中文词库拼「形容词动物」（无分隔，如"满速的仓鼠"），
  /// 否则用英文词库拼「Adj Animal」（空格分隔，如"Full-Speed Hamster"）。
  /// 形容词与动物各自独立随机抽取，不要求同下标。
  ///
  /// [seed] 缺省为空时用真随机；传入固定值可复现（单测 / 需要稳定结果的场景）。
  static String suggest(bool isZh, {int? seed}) {
    final rand = seed == null ? Random() : Random(seed);
    final adjectives = isZh ? adjectivesZh : adjectivesEn;
    final animals = isZh ? animalsZh : animalsEn;
    final adjective = adjectives[rand.nextInt(adjectives.length)];
    final animal = animals[rand.nextInt(animals.length)];
    return isZh ? '$adjective$animal' : '$adjective $animal';
  }
}
