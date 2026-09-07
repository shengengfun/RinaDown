import { motion } from "framer-motion";
import { Home, MessageSquarePlus } from "lucide-react";
import { useLocale } from "@/lib/i18n";

export default function NotFoundContent() {
  const { t } = useLocale();

  return (
    <section className="relative flex items-center justify-center min-h-[60vh] bg-dark-bg">
      <div className="mx-auto max-w-lg px-4 text-center">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
        >
          {/* 404 Number */}
          <h1 className="text-[120px] sm:text-[160px] font-bold leading-none tracking-tighter bg-gradient-to-b from-dark-text to-dark-text-muted bg-clip-text text-transparent select-none">
            404
          </h1>

          <h2 className="mt-2 text-xl sm:text-2xl font-semibold text-dark-text">
            {t("notFound.title")}
          </h2>
          <p className="mt-3 text-sm text-dark-text-secondary">
            {t("notFound.desc")}
          </p>

          {/* Actions */}
          <div className="mt-8 flex flex-col sm:flex-row items-center justify-center gap-3">
            <a
              href="/"
              className="inline-flex items-center gap-2 rounded-lg bg-brand-blue px-5 py-2.5 text-sm font-semibold text-white hover:bg-brand-blue/90 transition-colors shadow-lg shadow-brand-blue/20"
            >
              <Home className="w-4 h-4" />
              {t("notFound.home")}
            </a>
            <a
              href="/feedback"
              className="inline-flex items-center gap-2 rounded-lg border border-dark-border px-5 py-2.5 text-sm font-medium text-dark-text-secondary hover:text-dark-text hover:bg-dark-surface2 transition-colors"
            >
              <MessageSquarePlus className="w-4 h-4" />
              {t("notFound.feedback")}
            </a>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
