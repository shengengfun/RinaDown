import { useState, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useLocale } from "@/lib/i18n";
import { ANNOUNCEMENTS } from "@/lib/announcements";
import type { Announcement } from "@/lib/announcements";

const STORAGE_KEY = "fluxdown-dismissed-announcements";

/** 浮层延迟入场，避免与首屏内容争夺注意力 */
const APPEAR_DELAY_MS = 1800;

function getDismissed(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function setDismissed(ids: string[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(ids));
  } catch {
    // localStorage unavailable
  }
}

/**
 * 公告浮动卡片（历史原因文件名叫 Bar）：右上角（导航下方）延迟滑入，
 * 不遮挡内容、可关闭并记住。右下角被 CommunityFloat 占用。
 */
export default function AnnouncementBar() {
  const { t } = useLocale();
  const [visible, setVisible] = useState(false);
  const [current, setCurrent] = useState<Announcement | null>(null);

  useEffect(() => {
    const dismissed = getDismissed();
    const active = ANNOUNCEMENTS.filter(
      (a) => a.active && !dismissed.includes(a.id),
    ).sort((a, b) => b.date.localeCompare(a.date))[0];
    if (!active) return;
    setCurrent(active);
    const timer = setTimeout(() => setVisible(true), APPEAR_DELAY_MS);
    return () => clearTimeout(timer);
  }, []);

  const handleDismiss = useCallback(() => {
    if (!current) return;
    setVisible(false);
    const dismissed = getDismissed();
    setDismissed([...dismissed, current.id]);
  }, [current]);

  return (
    <AnimatePresence>
      {visible && current && (
        <motion.div
          initial={{ opacity: 0, x: 32, scale: 0.97 }}
          animate={{ opacity: 1, x: 0, scale: 1 }}
          exit={{ opacity: 0, x: 24, scale: 0.97 }}
          transition={{ duration: 0.4, ease: [0.22, 1, 0.36, 1] }}
          className="fixed top-20 right-4 left-4 sm:left-auto sm:right-6 sm:max-w-[360px] z-[60]"
          role="status"
        >
          <div className="relative rounded-xl border border-dark-border bg-dark-bg/95 backdrop-blur-xl shadow-xl shadow-black/20 p-4">
            <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-brand-sky/50 to-transparent" />

            <div className="flex items-start gap-3">
              <span className="relative flex h-2 w-2 shrink-0 mt-1.5">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-brand-sky opacity-75" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-brand-sky" />
              </span>

              <p className="text-[13px] text-dark-text-secondary leading-relaxed pr-5">
                {t(current.messageKey)}
              </p>

              <button
                onClick={handleDismiss}
                className="absolute top-3 right-3 flex items-center justify-center w-6 h-6 rounded-full hover:bg-dark-surface3/50 transition-colors cursor-pointer"
                aria-label={t("announcement.close")}
              >
                <svg
                  width="13"
                  height="13"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  className="text-dark-text-muted"
                >
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            </div>

            {current.link && (
              <div className="mt-3 flex justify-end">
                <a
                  href={current.link}
                  onClick={handleDismiss}
                  className="inline-flex items-center gap-1.5 rounded-lg bg-gradient-to-r from-brand-sky to-brand-cyan px-3.5 py-1.5 text-xs font-semibold text-white hover:opacity-90 transition-opacity"
                >
                  {t("announcement.cta")}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M5 12h14" />
                    <path d="m12 5 7 7-7 7" />
                  </svg>
                </a>
              </div>
            )}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
