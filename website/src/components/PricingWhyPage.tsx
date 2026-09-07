import { motion } from "framer-motion";
import { useLocale } from "@/lib/i18n";
import type { Messages } from "@/lib/locales";

/** 免费版 / 高级版对比行 */
const COMPARE_ROWS: {
  labelKey: keyof Messages;
  freeKey: keyof Messages;
  paidKey: keyof Messages;
}[] = [
  { labelKey: "pricing.why.row1Label", freeKey: "pricing.why.row1Free", paidKey: "pricing.why.row1Paid" },
  { labelKey: "pricing.why.row2Label", freeKey: "pricing.why.row2Free", paidKey: "pricing.why.row2Paid" },
  { labelKey: "pricing.why.row3Label", freeKey: "pricing.why.row3Free", paidKey: "pricing.why.row3Paid" },
  { labelKey: "pricing.why.row4Label", freeKey: "pricing.why.row4Free", paidKey: "pricing.why.row4Paid" },
  { labelKey: "pricing.why.row5Label", freeKey: "pricing.why.row5Free", paidKey: "pricing.why.row5Paid" },
  { labelKey: "pricing.why.row6Label", freeKey: "pricing.why.row6Free", paidKey: "pricing.why.row6Paid" },
  { labelKey: "pricing.why.row7Label", freeKey: "pricing.why.row7Free", paidKey: "pricing.why.row7Paid" },
];

const SECTIONS: { titleKey: keyof Messages; paragraphKeys: (keyof Messages)[] }[] = [
  { titleKey: "pricing.why.s1t", paragraphKeys: ["pricing.why.s1p1", "pricing.why.s1p2"] },
  { titleKey: "pricing.why.s2t", paragraphKeys: ["pricing.why.s2p1", "pricing.why.s2p2"] },
];

export default function PricingWhyPage() {
  const { t } = useLocale();

  return (
    <section className="pt-24 sm:pt-32 pb-16 sm:pb-20">
      <div className="mx-auto max-w-3xl px-4 sm:px-6 lg:px-8">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
        >
          <a
            href="/pricing"
            className="inline-flex items-center gap-1.5 text-sm text-dark-text-secondary hover:text-brand-sky transition-colors"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M19 12H5" />
              <path d="m12 19-7-7 7-7" />
            </svg>
            {t("pricing.why.backLink")}
          </a>

          <span className="mt-8 flex">
            <span className="inline-flex items-center gap-2 rounded-full border border-dark-border bg-dark-surface1/50 px-4 py-1.5 text-xs font-medium text-dark-text-secondary backdrop-blur-sm">
              {t("pricing.why.badge")}
            </span>
          </span>

          <h1 className="mt-6 text-3xl sm:text-4xl font-bold tracking-tight leading-tight text-dark-text">
            {t("pricing.why.title")}
          </h1>

          <p className="mt-4 text-base text-dark-text-secondary leading-relaxed">
            {t("pricing.why.intro")}
          </p>

          <p className="mt-4 rounded-lg border border-brand-sky/20 bg-brand-sky/[0.06] px-4 py-3 text-sm text-dark-text-secondary leading-relaxed">
            {t("pricing.why.draftNote")}
          </p>
        </motion.div>

        <motion.article
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5, delay: 0.15 }}
          className="mt-12 space-y-10"
        >
          {SECTIONS.map((s) => (
            <div key={s.titleKey}>
              <h2 className="text-xl sm:text-2xl font-semibold text-dark-text tracking-tight">
                {t(s.titleKey)}
              </h2>
              {s.paragraphKeys.map((pk) => (
                <p key={pk} className="mt-4 text-[15px] text-dark-text-secondary leading-relaxed">
                  {t(pk)}
                </p>
              ))}
            </div>
          ))}

          {/* 免费 / 高级对比 */}
          <div>
            <h2 className="text-xl sm:text-2xl font-semibold text-dark-text tracking-tight">
              {t("pricing.why.s3t")}
            </h2>
            <p className="mt-4 text-[15px] text-dark-text-secondary leading-relaxed">
              {t("pricing.why.s3p1")}
            </p>

            <div className="mt-6 rounded-xl border border-dark-border overflow-hidden">
              <div className="grid grid-cols-3 bg-dark-surface1/50 text-xs font-semibold uppercase tracking-wider text-dark-text-muted">
                <div className="px-4 py-3"></div>
                <div className="px-4 py-3">{t("pricing.why.tableFree")}</div>
                <div className="px-4 py-3 text-brand-sky">{t("pricing.why.tablePaid")}</div>
              </div>
              {COMPARE_ROWS.map((row, i) => (
                <div
                  key={row.labelKey}
                  className={`grid grid-cols-3 text-sm ${i % 2 === 1 ? "bg-dark-surface1/20" : ""} border-t border-dark-border/60`}
                >
                  <div className="px-4 py-3 font-medium text-dark-text">{t(row.labelKey)}</div>
                  <div className="px-4 py-3 text-dark-text-secondary">{t(row.freeKey)}</div>
                  <div className="px-4 py-3 text-dark-text-secondary">{t(row.paidKey)}</div>
                </div>
              ))}
            </div>

            <p className="mt-4 text-[15px] text-dark-text-secondary leading-relaxed">
              {t("pricing.why.s3p2")}
            </p>

            {/* 自建路径承诺 */}
            <div className="mt-5 rounded-xl border border-success/25 bg-success/[0.06] px-4 py-4">
              <p className="text-sm font-semibold text-success">
                {t("pricing.why.selfHostTitle")}
              </p>
              <p className="mt-2 text-[15px] text-dark-text-secondary leading-relaxed">
                {t("pricing.why.selfHostBody")}
              </p>
            </div>
          </div>

          {/* 参与决定 */}
          <div>
            <h2 className="text-xl sm:text-2xl font-semibold text-dark-text tracking-tight">
              {t("pricing.why.s4t")}
            </h2>
            <p className="mt-4 text-[15px] text-dark-text-secondary leading-relaxed">
              {t("pricing.why.s4p1")}
            </p>
            <a
              href="/pricing"
              className="mt-6 inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-brand-sky to-brand-cyan px-5 py-2.5 text-sm font-semibold text-white hover:opacity-90 transition-opacity"
            >
              {t("pricing.why.cta")}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M5 12h14" />
                <path d="m12 5 7 7-7 7" />
              </svg>
            </a>
          </div>
        </motion.article>
      </div>
    </section>
  );
}
