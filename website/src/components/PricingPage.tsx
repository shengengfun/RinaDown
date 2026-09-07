import { useState, useEffect } from "react";
import { motion } from "framer-motion";
import { useLocale } from "@/lib/i18n";
import type { Messages } from "@/lib/locales";
import WebPurchase from "./PricingWebPurchase";

/** FluxCloud 公开套餐目录条目（GET /api/cloud/plans，wire camelCase）。 */
interface CloudCampaign {
  name: string;
  endAt: string | null;
  stages: { label: string; priceMinor: number; quota: number | null }[];
  soldTotal: number;
  stageSold: number[];
  currentStageIndex: number;
  effectivePriceMinor: number;
}

interface CloudPlan {
  code: string;
  name: string;
  description: string;
  badge: string | null;
  badgeStyle: string;
  badgeColor: string;
  badgeNumbered: boolean;
  badgeNumberDigits: number;
  icon: string;
  color: string;
  priceMinor: number;
  currency: string;
  highlights: string[];
  /** 缺省视为可购买，兼容旧目录响应。下架但仍展示的套餐为 false。 */
  purchasable?: boolean;
  campaign: CloudCampaign | null;
}

/** 徽标前置图标：认证/勋章语义的简笔圆形对勾，纯描边，不依赖渐变。 */
function BadgeCheckIcon({ className }: { className?: string }) {
  return (
    <svg
      width="11"
      height="11"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={`shrink-0 ${className ?? ""}`}
    >
      <circle cx="12" cy="12" r="9" />
      <path d="m8.5 12.5 2.5 2.5 4.5-5" />
    </svg>
  );
}

/**
 * 套餐徽标（行内 pill）：按 badgeStyle 渲染 outline / solid / medal 三种形态，
 * 颜色一律来自 badgeColor（Tailwind 任意值类无法覆盖运营自定义 hex，必须内联 style）。
 * ribbon 需要卡片级绝对定位，这里返回 null，改由 PlanBadgeRibbon 在卡片容器渲染，
 * 避免重复展示。官网无登录态/个人编号上下文：badgeNumbered 即使为 true 也不展示
 * 具体编号，只展示徽标文案本身。
 */
function PlanBadgePill({ plan }: { plan: CloudPlan }) {
  if (!plan.badge || plan.badgeStyle === "ribbon") return null;
  const color = plan.badgeColor;
  const label = plan.badge;

  if (plan.badgeStyle === "solid") {
    return (
      <span
        className="inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[11px] font-semibold text-white"
        style={{ backgroundColor: color }}
      >
        <BadgeCheckIcon />
        {label}
      </span>
    );
  }

  if (plan.badgeStyle === "medal") {
    return (
      <span
        className="inline-flex items-stretch overflow-hidden rounded-full border text-[11px] font-semibold"
        style={{ borderColor: color }}
      >
        <span className="flex items-center px-1.5 text-white" style={{ backgroundColor: color }}>
          <BadgeCheckIcon />
        </span>
        <span className="flex items-center px-2 py-0.5" style={{ color, backgroundColor: `${color}1a` }}>
          {label}
        </span>
      </span>
    );
  }

  // outline：默认样式的强化版，更粗描边 + 图标
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full border-[1.5px] px-2.5 py-0.5 text-[11px] font-semibold"
      style={{ borderColor: color, color, backgroundColor: `${color}1a` }}
    >
      <BadgeCheckIcon />
      {label}
    </span>
  );
}

/** ribbon 样式：卡片右上角斜切色带，依赖卡片容器自身的 relative + overflow-hidden。 */
function PlanBadgeRibbon({ plan }: { plan: CloudPlan }) {
  if (!plan.badge || plan.badgeStyle !== "ribbon") return null;
  return (
    <div
      className="pointer-events-none absolute right-[-34px] top-[14px] z-10 w-[130px] rotate-45 py-1 text-center text-[10px] font-bold tracking-wide text-white shadow-sm"
      style={{ backgroundColor: plan.badgeColor }}
    >
      {plan.badge}
    </div>
  );
}

function formatPrice(minor: number, currency: string, locale: string): string {
  try {
    return new Intl.NumberFormat(locale === "zh" ? "zh-CN" : locale, {
      style: "currency",
      currency,
      minimumFractionDigits: minor % 100 === 0 ? 0 : 2,
    }).format(minor / 100);
  } catch {
    return `${(minor / 100).toFixed(2)} ${currency}`;
  }
}

/** 云端目录不可达时的静态占位卡片（价格待定）。 */
const FALLBACK_CARDS: {
  key: string;
  nameKey: keyof Messages;
  descKey: keyof Messages;
  featureKeys: (keyof Messages)[];
}[] = [
  {
    key: "free",
    nameKey: "pricing.fallbackFreeName",
    descKey: "pricing.fallbackFreeDesc",
    featureKeys: ["pricing.subF1", "pricing.subF2"],
  },
  {
    key: "lifetime",
    nameKey: "pricing.lifetimeName",
    descKey: "pricing.lifetimeDesc",
    featureKeys: [
      "pricing.lifetimeF1",
      "pricing.lifetimeF2",
      "pricing.lifetimeF3",
      "pricing.lifetimeF4",
    ],
  },
];

/** 定价模型说明：账户可选 / 一次买断 / 附加另购。 */
const MODEL_NOTES: {
  titleKey: keyof Messages;
  descKey: keyof Messages;
  icon: React.ReactNode;
}[] = [
  {
    titleKey: "pricing.noteNoAccountTitle",
    descKey: "pricing.noteNoAccountDesc",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" />
        <circle cx="12" cy="7" r="4" />
      </svg>
    ),
  },
  {
    titleKey: "pricing.noteBuyoutTitle",
    descKey: "pricing.noteBuyoutDesc",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M20 6 9 17l-5-5" />
      </svg>
    ),
  },
  {
    titleKey: "pricing.noteScopeTitle",
    descKey: "pricing.noteScopeDesc",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="m7.5 4.27 9 5.15" />
        <path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4a2 2 0 0 0 1-1.73Z" />
        <path d="M3.3 7 12 12l8.7-5" />
        <path d="M12 22V12" />
      </svg>
    ),
  },
];

export default function PricingPage() {
  const { t, locale } = useLocale();
  const [cloudPlans, setCloudPlans] = useState<CloudPlan[]>([]);

  useEffect(() => {
    // FluxCloud 动态套餐目录：失败静默降级为静态占位卡片。
    fetch("/api/cloud/plans")
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then((plans: CloudPlan[]) => {
        if (Array.isArray(plans)) setCloudPlans(plans);
      })
      .catch(() => {});
  }, []);

  return (
    <section className="pt-24 sm:pt-32 pb-16 sm:pb-20">
      <div className="mx-auto max-w-6xl px-4 sm:px-6 lg:px-8">
        {/* ── Header ── */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="text-center mb-12 sm:mb-16"
        >
          <span className="inline-flex items-center gap-2 rounded-full border border-dark-border bg-dark-surface1/50 px-4 py-1.5 text-xs font-medium text-dark-text-secondary backdrop-blur-sm mb-6">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-brand-sky">
              <path d="M12 1v22" />
              <path d="M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6" />
            </svg>
            {t("pricing.badge")}
          </span>

          <h1 className="text-4xl sm:text-5xl font-bold tracking-tight leading-tight sm:whitespace-nowrap">
            <span className="text-dark-text">{t("pricing.title")}</span>
            <span className="bg-gradient-to-r from-brand-sky to-brand-cyan bg-clip-text text-transparent">{t("pricing.titleHighlight")}</span>
          </h1>

          <p className="mt-4 text-base sm:text-lg text-dark-text-secondary max-w-2xl mx-auto leading-relaxed">
            {t("pricing.subtitle")}
          </p>
          <a
            href="/pricing/why"
            className="mt-5 inline-flex items-center gap-1.5 text-sm font-medium text-brand-sky hover:text-brand-cyan transition-colors"
          >
            {t("pricing.whyLink")}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M5 12h14" />
              <path d="m12 5 7 7-7 7" />
            </svg>
          </a>
        </motion.div>

        {/* ── Plan cards：优先 FluxCloud 动态目录，云端不可达时回退静态占位 ── */}
        {cloudPlans.length > 0 ? (
          <div
            className={`grid grid-cols-1 sm:grid-cols-2 ${
              cloudPlans.length >= 3 ? "lg:grid-cols-3 max-w-6xl" : "max-w-3xl"
            } gap-5 sm:gap-6 mx-auto items-stretch`}
          >
            {cloudPlans.map((plan, i) => {
              const c = plan.campaign;
              const stage = c ? c.stages[c.currentStageIndex] : null;
              const stageLeft =
                c && stage && stage.quota != null
                  ? Math.max(0, stage.quota - (c.stageSold[c.currentStageIndex] ?? 0))
                  : null;
              const hasDiscount = c != null && c.effectivePriceMinor < plan.priceMinor;
              const priceMinor = c ? c.effectivePriceMinor : plan.priceMinor;
              const purchasable = plan.purchasable !== false;
              const featured = plan.priceMinor > 0;
              return (
                <motion.div
                  key={plan.code}
                  initial={{ opacity: 0, y: 20 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ duration: 0.4, delay: 0.1 * i }}
                  className={`relative flex flex-col rounded-2xl border overflow-hidden ${
                    featured
                      ? "border-brand-sky/40 bg-dark-surface1/50 shadow-[0_0_40px_-12px_rgba(56,189,248,0.25)]"
                      : "border-dark-border bg-dark-surface1/30"
                  }`}
                >
                  <PlanBadgeRibbon plan={plan} />
                  {/* 活动横幅：占据卡片顶部整条，避免碎片化徽章堆叠 */}
                  {c && (
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-6 sm:px-8 py-2.5 bg-brand-sky/10 border-b border-brand-sky/20 text-xs">
                      <span className="inline-flex items-center gap-1.5 font-medium text-brand-sky whitespace-nowrap">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
                        </svg>
                        {c.name}
                      </span>
                      {stage?.label && (
                        <span className="text-dark-text-secondary/80 whitespace-nowrap">{stage.label}</span>
                      )}
                      {stageLeft != null && (
                        <span className="ml-auto text-dark-text-secondary tabular-nums whitespace-nowrap">
                          {t("pricing.campaignLeft", { n: String(stageLeft) })}
                        </span>
                      )}
                    </div>
                  )}
                  <div className="flex flex-col flex-1 p-6 sm:p-8">
                    <div className="flex items-center gap-2.5">
                      <h2 className="text-lg font-semibold text-dark-text">{plan.name}</h2>
                      {plan.badge && <PlanBadgePill plan={plan} />}
                    </div>
                    <div className="mt-4 flex items-baseline gap-2.5">
                      <span className="text-4xl font-bold tracking-tight text-dark-text tabular-nums">
                        {priceMinor === 0
                          ? t("pricing.cloudFree")
                          : formatPrice(priceMinor, plan.currency, locale)}
                      </span>
                      {hasDiscount && (
                        <span className="text-base text-dark-text-muted line-through tabular-nums">
                          {formatPrice(plan.priceMinor, plan.currency, locale)}
                        </span>
                      )}
                      {priceMinor > 0 && (
                        <span className="text-sm text-dark-text-muted">{t("pricing.oneTime")}</span>
                      )}
                    </div>
                    {c?.endAt && (
                      <p className="mt-1.5 text-xs text-dark-text-muted">
                        {t("pricing.campaignEnds", {
                          date: new Date(c.endAt).toLocaleDateString(),
                        })}
                      </p>
                    )}
                    {plan.description && (
                      <p className="mt-4 text-sm text-dark-text-muted leading-relaxed">
                        {plan.description}
                      </p>
                    )}
                    {plan.highlights.length > 0 && (
                      <ul className="mt-6 space-y-2.5">
                        {plan.highlights.map((h) => (
                          <li key={h} className="flex items-start gap-2.5 text-sm text-dark-text-secondary">
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="mt-0.5 shrink-0 text-brand-sky">
                              <polyline points="20 6 9 17 4 12" />
                            </svg>
                            {h}
                          </li>
                        ))}
                      </ul>
                    )}
                    {featured && (
                      <div className="mt-auto pt-6">
                        <div className="border-t border-dark-border pt-4">
                          {purchasable ? (
                            <>
                              <WebPurchase
                                plan={{
                                  code: plan.code,
                                  name: plan.name,
                                  priceMinor,
                                  currency: plan.currency,
                                }}
                              />
                              <p className="mt-2.5 text-xs text-dark-text-muted text-center">
                                {t("pricing.buyInApp")}
                              </p>
                            </>
                          ) : (
                            <p className="text-center text-sm text-dark-text-muted">
                              {t("pricing.unavailable")}
                            </p>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                </motion.div>
              );
            })}
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 sm:gap-6 max-w-3xl mx-auto items-stretch">
            {FALLBACK_CARDS.map((plan, i) => (
              <motion.div
                key={plan.key}
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.4, delay: 0.1 * i }}
                className={`relative flex flex-col rounded-2xl border overflow-hidden ${
                  plan.key === "lifetime"
                    ? "border-brand-sky/40 bg-dark-surface1/50"
                    : "border-dark-border bg-dark-surface1/30"
                }`}
              >
                <div className="flex flex-col flex-1 p-6 sm:p-8">
                  <h2 className="text-lg font-semibold text-dark-text">{t(plan.nameKey)}</h2>
                  <div className="mt-4 flex items-baseline gap-2">
                    <span className="text-4xl font-bold tracking-tight text-dark-text">
                      {plan.key === "free" ? t("pricing.cloudFree") : t("pricing.tbd")}
                    </span>
                    {plan.key === "lifetime" && (
                      <span className="text-sm text-dark-text-muted">{t("pricing.oneTime")}</span>
                    )}
                  </div>
                  <p className="mt-4 text-sm text-dark-text-muted leading-relaxed">
                    {t(plan.descKey)}
                  </p>
                  <ul className="mt-6 space-y-2.5">
                    {plan.featureKeys.map((fk) => (
                      <li key={fk} className="flex items-start gap-2.5 text-sm text-dark-text-secondary">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="mt-0.5 shrink-0 text-brand-sky">
                          <polyline points="20 6 9 17 4 12" />
                        </svg>
                        {t(fk)}
                      </li>
                    ))}
                  </ul>
                </div>
              </motion.div>
            ))}
          </div>
        )}

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.3 }}
          className="mt-6 text-center max-w-2xl mx-auto"
        >
          <p className="text-sm text-dark-text-muted">{t("pricing.freeNote")}</p>
        </motion.div>

        {/* ── 定价模型说明：账户可选 / 一次买断 / 附加另购 ── */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.5 }}
          className="mt-16 sm:mt-20 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4 max-w-5xl mx-auto"
        >
          {MODEL_NOTES.map((note) => (
            <div
              key={note.titleKey}
              className="rounded-xl border border-dark-border bg-dark-surface1/30 p-5 sm:p-6"
            >
              <div className="flex items-center gap-2.5 text-brand-sky">
                {note.icon}
                <h3 className="text-sm font-semibold text-dark-text">{t(note.titleKey)}</h3>
              </div>
              <p className="mt-2.5 text-sm text-dark-text-muted leading-relaxed">
                {t(note.descKey)}
              </p>
            </div>
          ))}
        </motion.div>

        {/* ── 投票与讨论入口（已迁移至独立页面）── */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.5 }}
          className="mt-12 sm:mt-16 max-w-4xl mx-auto"
        >
          <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 rounded-xl border border-dark-border bg-dark-surface1/30 p-5 sm:p-6">
            <div>
              <h3 className="text-sm font-semibold text-dark-text">{t("pricing.voteCtaTitle")}</h3>
              <p className="mt-1.5 text-sm text-dark-text-muted leading-relaxed">
                {t("pricing.voteCtaDesc")}
              </p>
            </div>
            <a
              href="/pricing/vote"
              className="shrink-0 inline-flex items-center gap-1.5 rounded-lg border border-dark-border px-4 py-2 text-sm font-medium text-dark-text hover:border-brand-sky/50 hover:text-brand-sky transition-colors"
            >
              {t("pricing.voteCtaLink")}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M5 12h14" />
                <path d="m12 5 7 7-7 7" />
              </svg>
            </a>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
