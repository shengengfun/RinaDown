// 默认昵称建议 —— "形容词+动物"组合，速度×囤积主题（呼应下载圈"仓鼠党"梗），
// 中英词库顺序对应、数组索引即为组合关系。用于：
// - 注册表单预填 + 🎲 换一换（纯随机）；
// - 验证码登录命中"邮箱不存在→自动注册"时，客户端恒传一个建议昵称（服务端仅在
//   自动注册新用户时采用，已存在用户忽略，见 client.ts codeVerify）。
// 服务端自动注册的默认昵称走同一份词库（见 FluxCloud/server），保证观感一致。

const ADJECTIVES_ZH = ['满速的', '囤货的', '蹲种的', '疾风的', '通宵的', '爆仓的', '拆包的', '追更的', '破浪的', '闪电的', '挂机的', '起飞的']
const ADJECTIVES_EN = ['Full-Speed', 'Hoarding', 'Seeding', 'Gale', 'All-Night', 'Overstocked', 'Unboxing', 'Binge', 'Wavebreaker', 'Lightning', 'Idle', 'Takeoff']
const ANIMALS_ZH = ['仓鼠', '浣熊', '企鹅', '隼', '树懒', '松鼠', '信天翁', '旗鱼', '雪貂', '狸猫', '蜂鸟', '水獭']
const ANIMALS_EN = ['Hamster', 'Raccoon', 'Penguin', 'Falcon', 'Sloth', 'Squirrel', 'Albatross', 'Sailfish', 'Ferret', 'Tanuki', 'Hummingbird', 'Otter']

interface NicknameBank {
  adjectives: string[]
  animals: string[]
  /** 组合方式：中文形容词+动物紧贴，英文以空格分隔。 */
  join: (adjective: string, animal: string) => string
}

const BANKS: Record<'zh' | 'en', NicknameBank> = {
  zh: { adjectives: ADJECTIVES_ZH, animals: ANIMALS_ZH, join: (adj, animal) => `${adj}${animal}` },
  en: { adjectives: ADJECTIVES_EN, animals: ANIMALS_EN, join: (adj, animal) => `${adj} ${animal}` },
}

/** 确定性字符串哈希（FNV-1a 变体），仅用于按 seed 稳定复现组合，非加密用途。 */
function hashSeed(seed: string): number {
  let h = 2166136261
  for (let i = 0; i < seed.length; i++) {
    h ^= seed.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}

/** 生成"形容词+动物"默认昵称建议。
 *  - locale：非 "zh" 一律回退英文词库（多语言按语系前缀已在 i18n 层归一，见 detectBrowserLocale）。
 *  - seed：传入时（如 Origin ID）确定性复现同一组合，便于同一账号观感稳定；
 *    不传则每次随机生成，用于注册表单预填 + 🎲 换一换。 */
export function suggest(locale: string, seed?: string | number): string {
  const bank = locale === 'zh' ? BANKS.zh : BANKS.en
  let adjIndex: number
  let animalIndex: number
  if (seed != null) {
    const h = hashSeed(String(seed))
    adjIndex = h % bank.adjectives.length
    animalIndex = Math.floor(h / bank.adjectives.length) % bank.animals.length
  } else {
    adjIndex = Math.floor(Math.random() * bank.adjectives.length)
    animalIndex = Math.floor(Math.random() * bank.animals.length)
  }
  return bank.join(bank.adjectives[adjIndex], bank.animals[animalIndex])
}
