import { motion } from "framer-motion";
import { useLocale } from "@/lib/i18n";
import { ANNOUNCEMENTS } from "@/lib/announcements";

export default function AnnouncementsPage() {
  const { t } = useLocale();

  const sorted = [...ANNOUNCEMENTS].sort((a, b) => {
    if (a.active !== b.active) return a.active ? -1 : 1;
    return b.date.localeCompare(a.date);
  });

  return (
    <section className="pt-24 sm:pt-32 pb-16 sm:pb-20">
      <div className="mx-auto max-w-3xl px-4 sm:px-6 lg:px-8">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="text-center mb-12 sm:mb-16"
        >
          <span className="inline-flex items-center gap-2 rounded-full border border-dark-border bg-dark-surface1/50 px-4 py-1.5 text-xs font-medium text-dark-text-secondary backdrop-blur-sm mb-6">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-brand-sky">
              <path d="m3 11 18-5v12L3 13v-2Z" />
              <path d="M11.6 16.8a3 3 0 1 1-5.8-1.6" />
            </svg>
            {t("announcement.badge")}
          </span>

          <h1 className="text-4xl sm:text-5xl font-bold tracking-tight leading-tight">
            <span className="text-dark-text">{t("announcement.title")}</span>
            <span className="bg-gradient-to-r from-brand-sky to-brand-cyan bg-clip-text text-transparent">{t("announcement.titleHighlight")}</span>
          </h1>

          <p className="mt-4 text-base sm:text-lg text-dark-text-secondary max-w-2xl mx-auto leading-relaxed">
            {t("announcement.subtitle")}
          </p>
        </motion.div>

        {sorted.length === 0 && (
          <div className="text-center py-20">
            <span className="text-sm text-dark-text-muted">{t("announcement.empty")}</span>
          </div>
        )}

        <div className="space-y-4">
          {sorted.map((item, i) => (
            <motion.div
              key={item.id}
              initial={{ opacity: 0, y: 16 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.4, delay: 0.05 * i }}
            >
              <a
                href={item.link || "#"}
                className={`group block rounded-xl border overflow-hidden transition-all duration-300 ${
                  item.active
                    ? "border-dark-border hover:border-dark-text-muted hover:shadow-lg hover:shadow-black/10"
                    : "border-dark-border/50 opacity-60"
                }`}
              >
                <div className="relative p-5 sm:p-6">
                  <div className="absolute inset-0 bg-gradient-to-r from-brand-blue/5 via-transparent to-brand-cyan/5 opacity-0 group-hover:opacity-100 transition-opacity duration-300" />

                  <div className="relative flex items-start gap-4">
                    <div className="shrink-0 mt-0.5 flex items-center justify-center w-10 h-10 rounded-lg bg-dark-surface2">
                      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className={item.active ? "text-brand-sky" : "text-dark-text-muted"}>
                        <path d="m3 11 18-5v12L3 13v-2Z" />
                        <path d="M11.6 16.8a3 3 0 1 1-5.8-1.6" />
                      </svg>
                    </div>

                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 mb-2">
                        <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium ${
                          item.active
                            ? "bg-success/10 text-success border border-success/20"
                            : "bg-dark-surface3 text-dark-text-muted border border-dark-border"
                        }`}>
                          {item.active && (
                            <span className="relative flex h-1.5 w-1.5">
                              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-success opacity-75" />
                              <span className="relative inline-flex h-1.5 w-1.5 rounded-full bg-success" />
                            </span>
                          )}
                          {item.active ? t("announcement.active") : t("announcement.ended")}
                        </span>
                        <span className="text-xs text-dark-text-muted tabular-nums">{item.date}</span>
                      </div>

                      <p className="text-sm sm:text-base text-dark-text leading-relaxed">
                        {t(item.messageKey)}
                      </p>
                    </div>

                    {item.link && (
                      <div className="shrink-0 flex items-center self-center">
                        <span className="text-xs text-dark-text-muted group-hover:text-dark-text-secondary transition-colors hidden sm:inline mr-1">
                          {t("announcement.viewDetail")}
                        </span>
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-dark-text-muted group-hover:text-dark-text-secondary group-hover:translate-x-0.5 transition-all">
                          <polyline points="9 18 15 12 9 6" />
                        </svg>
                      </div>
                    )}
                  </div>
                </div>
              </a>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}
