import { motion } from "framer-motion";
import { useLocale } from "@/lib/i18n";
import type { Messages } from "@/lib/locales";
import { GITHUB_REPO_URL } from "@/lib/utils";

/** 与 AnnouncementModal 保持一致的官方域名列表；首项为主域名 */
const OFFICIAL_SITES = [
  "https://www.fluxdown.com",
  "https://fluxdown.com",
  "https://fluxdown.zerx.dev",
] as const;

const ADVICE_KEYS: (keyof Messages)[] = [
  "securityAlert.advice1",
  "securityAlert.advice2",
  "securityAlert.advice3",
];

export default function SecurityAlertPage() {
  const { t } = useLocale();

  return (
    <section className="pt-24 sm:pt-32 pb-16 sm:pb-20">
      <div className="mx-auto max-w-3xl px-4 sm:px-6 lg:px-8">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
        >
          <div className="flex items-center gap-3">
            <div className="shrink-0 flex items-center justify-center w-11 h-11 rounded-full bg-destructive/15 border border-destructive/30">
              <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-destructive">
                <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z" />
                <line x1="12" y1="9" x2="12" y2="13" />
                <line x1="12" y1="17" x2="12.01" y2="17" />
              </svg>
            </div>
            <h1 className="text-3xl sm:text-4xl font-bold tracking-tight text-dark-text">
              {t("announcementModal.title")}
            </h1>
          </div>

          <p className="mt-6 text-base text-dark-text-secondary leading-relaxed">
            {t("announcementModal.body")}
          </p>

          <div className="mt-8 space-y-3">
            <div className="rounded-xl border border-destructive/25 bg-destructive/[0.06] px-4 py-3">
              <p className="text-[11px] font-medium uppercase tracking-wide text-destructive/90">
                {t("announcementModal.fakeLabel")}
              </p>
              <p className="mt-1 font-mono text-sm text-dark-text line-through decoration-destructive/60">
                {t("announcementModal.fakeSite")}
              </p>
            </div>

            <div className="rounded-xl border border-success/25 bg-success/[0.06] px-4 py-3">
              <p className="text-[11px] font-medium uppercase tracking-wide text-success/90">
                {t("announcementModal.officialLabel")}
              </p>
              {OFFICIAL_SITES.map((site) => (
                <a
                  key={site}
                  href={site}
                  className="mt-1 flex w-fit items-center gap-1.5 font-mono text-sm text-success hover:underline"
                >
                  {site}
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M7 7h10v10" />
                    <path d="M7 17 17 7" />
                  </svg>
                </a>
              ))}
            </div>
          </div>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5, delay: 0.15 }}
          className="mt-12 space-y-10"
        >
          <div>
            <h2 className="text-xl sm:text-2xl font-semibold text-dark-text tracking-tight">
              {t("securityAlert.adviceTitle")}
            </h2>
            <ol className="mt-4 space-y-3">
              {ADVICE_KEYS.map((k, i) => (
                <li key={k} className="flex items-start gap-3 text-[15px] text-dark-text-secondary leading-relaxed">
                  <span className="shrink-0 flex items-center justify-center w-6 h-6 mt-0.5 rounded-full bg-dark-surface2 text-xs font-semibold text-dark-text tabular-nums">
                    {i + 1}
                  </span>
                  {t(k)}
                </li>
              ))}
            </ol>
          </div>

          <div>
            <h2 className="text-xl sm:text-2xl font-semibold text-dark-text tracking-tight">
              {t("securityAlert.downloadTitle")}
            </h2>
            <p className="mt-4 text-[15px] text-dark-text-secondary leading-relaxed">
              {t("securityAlert.downloadNote")}
            </p>
            <div className="mt-6 flex flex-wrap gap-3">
              <a
                href="/#download"
                className="inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-brand-sky to-brand-cyan px-5 py-2.5 text-sm font-semibold text-white hover:opacity-90 transition-opacity"
              >
                {t("securityAlert.downloadCta")}
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M5 12h14" />
                  <path d="m12 5 7 7-7 7" />
                </svg>
              </a>
              <a
                href={`${GITHUB_REPO_URL}/releases`}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 rounded-lg border border-dark-border px-5 py-2.5 text-sm font-medium text-dark-text-secondary hover:bg-dark-surface2 hover:text-dark-text transition-colors"
              >
                <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.58.11.79-.25.79-.55 0-.27-.01-1.17-.02-2.12-3.2.7-3.87-1.36-3.87-1.36-.52-1.33-1.28-1.68-1.28-1.68-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.19 1.76 1.19 1.03 1.76 2.69 1.25 3.35.96.1-.75.4-1.25.72-1.54-2.55-.29-5.24-1.28-5.24-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.51-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11.04 11.04 0 0 1 5.78 0c2.2-1.49 3.17-1.18 3.17-1.18.62 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.69 5.38-5.26 5.67.41.35.77 1.05.77 2.12 0 1.53-.01 2.76-.01 3.14 0 .3.21.67.8.55A11.51 11.51 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5z" />
                </svg>
                GitHub Releases
              </a>
            </div>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
