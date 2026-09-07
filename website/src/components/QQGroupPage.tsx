import { useState, useCallback } from "react";
import { motion } from "framer-motion";
import { useLocale } from "@/lib/i18n";

const QQ_GROUP_NUMBER = "832143651";

export default function QQGroupPage() {
  const { t } = useLocale();
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(QQ_GROUP_NUMBER);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      const input = document.createElement("input");
      input.value = QQ_GROUP_NUMBER;
      document.body.appendChild(input);
      input.select();
      document.execCommand("copy");
      document.body.removeChild(input);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }, []);

  return (
    <section className="pt-24 sm:pt-32 pb-16 sm:pb-20">
      <div className="mx-auto max-w-2xl px-4 sm:px-6 lg:px-8">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="text-center mb-12 sm:mb-16"
        >
          <span className="inline-flex items-center gap-2 rounded-full border border-dark-border bg-dark-surface1/50 px-4 py-1.5 text-xs font-medium text-dark-text-secondary backdrop-blur-sm mb-6">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-[#12B7F5]">
              <path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5Z" />
            </svg>
            {t("qqGroup.badge")}
          </span>

          <h1 className="text-4xl sm:text-5xl font-bold tracking-tight leading-tight">
            <span className="text-dark-text">{t("qqGroup.title")}</span>
            <span className="bg-gradient-to-r from-[#12B7F5] to-brand-cyan bg-clip-text text-transparent">{t("qqGroup.titleHighlight")}</span>
          </h1>

          <p className="mt-4 text-base sm:text-lg text-dark-text-secondary max-w-xl mx-auto leading-relaxed">
            {t("qqGroup.subtitle")}
          </p>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5, delay: 0.15 }}
          className="rounded-2xl border border-dark-border overflow-hidden"
        >
          <div className="relative p-6 sm:p-8">
            <div className="absolute inset-0 bg-gradient-to-b from-[#12B7F5]/10 to-transparent opacity-50" />

            <div className="relative flex flex-col items-center gap-6">
              <div className="rounded-xl border border-dark-border bg-white p-3 shadow-lg shadow-black/10">
                <img
                  src="/qq-group.png"
                  alt="QQ Group QR Code"
                  className="w-48 h-48 sm:w-56 sm:h-56 object-contain"
                />
              </div>

              <div className="flex flex-col items-center gap-3">
                <span className="text-sm text-dark-text-muted">{t("qqGroup.groupNumber")}</span>
                <div className="flex items-center gap-3">
                  <span className="text-2xl sm:text-3xl font-bold text-dark-text tabular-nums tracking-wider">
                    {QQ_GROUP_NUMBER}
                  </span>
                  <button
                    onClick={handleCopy}
                    className="flex items-center gap-1.5 rounded-lg border border-dark-border bg-dark-surface1/50 px-3 py-1.5 text-xs font-medium text-dark-text-secondary hover:text-dark-text hover:border-dark-text-muted transition-colors cursor-pointer"
                  >
                    {copied ? (
                      <>
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-success">
                          <polyline points="20 6 9 17 4 12" />
                        </svg>
                        {t("qqGroup.copied")}
                      </>
                    ) : (
                      <>
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
                          <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
                        </svg>
                        {t("qqGroup.copy")}
                      </>
                    )}
                  </button>
                </div>
              </div>

              <div className="w-full mt-2 rounded-xl border border-dark-border/50 bg-dark-surface1/30 p-4 sm:p-5">
                <h3 className="text-sm font-medium text-dark-text mb-3">{t("qqGroup.howToJoin")}</h3>
                <ol className="space-y-2 text-sm text-dark-text-secondary leading-relaxed">
                  <li className="flex gap-2">
                    <span className="shrink-0 flex items-center justify-center w-5 h-5 rounded-full bg-dark-surface3 text-[10px] font-semibold text-dark-text-muted">1</span>
                    {t("qqGroup.step1")}
                  </li>
                  <li className="flex gap-2">
                    <span className="shrink-0 flex items-center justify-center w-5 h-5 rounded-full bg-dark-surface3 text-[10px] font-semibold text-dark-text-muted">2</span>
                    {t("qqGroup.step2")}
                  </li>
                  <li className="flex gap-2">
                    <span className="shrink-0 flex items-center justify-center w-5 h-5 rounded-full bg-dark-surface3 text-[10px] font-semibold text-dark-text-muted">3</span>
                    {t("qqGroup.step3")}
                  </li>
                </ol>
              </div>
            </div>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
