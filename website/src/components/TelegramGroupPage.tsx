import { motion } from "framer-motion";
import { useLocale } from "@/lib/i18n";

const TELEGRAM_INVITE_LINK = "https://t.me/+tp8ie_FVjv02ZDNl";

export default function TelegramGroupPage() {
  const { t } = useLocale();

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
            {/* Telegram paper plane icon */}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" className="text-[#26A5E4]">
              <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"/>
            </svg>
            {t("telegramGroup.badge")}
          </span>

          <h1 className="text-4xl sm:text-5xl font-bold tracking-tight leading-tight">
            <span className="text-dark-text">{t("telegramGroup.title")}</span>
            <span className="bg-gradient-to-r from-[#26A5E4] to-brand-cyan bg-clip-text text-transparent">{t("telegramGroup.titleHighlight")}</span>
          </h1>

          <p className="mt-4 text-base sm:text-lg text-dark-text-secondary max-w-xl mx-auto leading-relaxed">
            {t("telegramGroup.subtitle")}
          </p>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5, delay: 0.15 }}
          className="rounded-2xl border border-dark-border overflow-hidden"
        >
          <div className="relative p-6 sm:p-8">
            <div className="absolute inset-0 bg-gradient-to-b from-[#26A5E4]/10 to-transparent opacity-50" />

            <div className="relative flex flex-col items-center gap-6">
              {/* Telegram logo */}
              <div className="flex items-center justify-center w-24 h-24 rounded-2xl bg-gradient-to-br from-[#26A5E4] to-[#1a8fcb] shadow-lg shadow-[#26A5E4]/20">
                <svg width="48" height="48" viewBox="0 0 24 24" fill="white">
                  <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"/>
                </svg>
              </div>

              {/* Join button */}
              <a
                href={TELEGRAM_INVITE_LINK}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2.5 rounded-xl bg-[#26A5E4] hover:bg-[#1a8fcb] text-white font-semibold px-8 py-3.5 text-base transition-colors shadow-lg shadow-[#26A5E4]/25 cursor-pointer"
              >
                <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"/>
                </svg>
                {t("telegramGroup.joinBtn")}
              </a>

              <div className="w-full mt-2 rounded-xl border border-dark-border/50 bg-dark-surface1/30 p-4 sm:p-5">
                <h3 className="text-sm font-medium text-dark-text mb-3">{t("telegramGroup.howToJoin")}</h3>
                <ol className="space-y-2 text-sm text-dark-text-secondary leading-relaxed">
                  <li className="flex gap-2">
                    <span className="shrink-0 flex items-center justify-center w-5 h-5 rounded-full bg-dark-surface3 text-[10px] font-semibold text-dark-text-muted">1</span>
                    {t("telegramGroup.step1")}
                  </li>
                  <li className="flex gap-2">
                    <span className="shrink-0 flex items-center justify-center w-5 h-5 rounded-full bg-dark-surface3 text-[10px] font-semibold text-dark-text-muted">2</span>
                    {t("telegramGroup.step2")}
                  </li>
                  <li className="flex gap-2">
                    <span className="shrink-0 flex items-center justify-center w-5 h-5 rounded-full bg-dark-surface3 text-[10px] font-semibold text-dark-text-muted">3</span>
                    {t("telegramGroup.step3")}
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
