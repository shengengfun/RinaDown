import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useLocale } from "@/lib/i18n";

const TELEGRAM_INVITE_LINK = "https://t.me/+tp8ie_FVjv02ZDNl";

const communities = [
  {
    id: "telegram",
    href: TELEGRAM_INVITE_LINK,
    external: true,
    labelKey: "community.telegram" as const,
    color: "#26A5E4",
    hoverColor: "#1a8fcb",
    shadowColor: "rgba(38,165,228,0.35)",
    icon: (
      <svg width="20" height="20" viewBox="0 0 24 24" fill="white">
        <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z" />
      </svg>
    ),
  },
  {
    id: "qq",
    href: "/qq-group",
    external: false,
    labelKey: "community.qq" as const,
    color: "#12B7F5",
    hoverColor: "#0ea5d9",
    shadowColor: "rgba(18,183,245,0.35)",
    icon: (
      <svg width="20" height="20" viewBox="0 0 24 24" fill="white">
        <path d="M21.395 15.035a39.548 39.548 0 0 0-.803-2.264 22.873 22.873 0 0 0-.442-1.010c.639-1.37.998-2.97.998-4.699C21.148 3.167 17.068 0 12.002 0 6.936 0 2.855 3.167 2.855 7.062c0 1.73.359 3.329.998 4.699-.134.302-.285.645-.442 1.010a39.548 39.548 0 0 0-.803 2.264C1.204 18.394 1.7 20.504 3.05 21.076c1.367.578 3.216-.61 4.647-2.68.522.155 1.084.27 1.668.34.416.823.929 1.528 1.508 2.07.468.437.959.695 1.454.695h.003c.494 0 .985-.258 1.452-.694.579-.543 1.092-1.248 1.508-2.071.584-.07 1.146-.185 1.668-.34 1.432 2.07 3.28 3.259 4.647 2.681 1.352-.572 1.847-2.682.538-6.042z" />
      </svg>
    ),
  },
  {
    id: "langbot",
    href: "#",
    external: false,
    labelKey: "community.bot" as const,
    color: "#2563eb",
    hoverColor: "#1d4ed8",
    shadowColor: "rgba(37,99,235,0.35)",
    icon: (
      <svg width="20" height="20" viewBox="0 0 24 24" fill="white">
        <path d="M20 9V7c0-1.1-.9-2-2-2h-3c0-1.66-1.34-3-3-3S9 3.34 9 5H6c-1.1 0-2 .9-2 2v2c-1.66 0-3 1.34-3 3s1.34 3 3 3v4c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2v-4c1.66 0 3-1.34 3-3s-1.34-3-3-3z" />
      </svg>
    ),
  },
];

export default function CommunityFloat() {
  const { t } = useLocale();
  const [open, setOpen] = useState(false);
  const visible = true;
  const ref = useRef<HTMLDivElement>(null);


  // 点击外部关闭
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          ref={ref}
          initial={{ opacity: 0, scale: 0.8, y: 16 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.8, y: 16 }}
          transition={{ type: "spring", stiffness: 380, damping: 28 }}
          className="fixed bottom-6 right-6 z-40 flex flex-col items-end gap-3"
        >
          {/* 子按钮列表 */}
          <AnimatePresence>
            {open &&
              communities.map((item, i) => (
                <motion.div
                  key={item.id}
                  initial={{ opacity: 0, scale: 0.7, y: 12 }}
                  animate={{ opacity: 1, scale: 1, y: 0 }}
                  exit={{ opacity: 0, scale: 0.7, y: 12 }}
                  transition={{
                    type: "spring",
                    stiffness: 400,
                    damping: 26,
                    delay: i * 0.05,
                  }}
                  className="flex items-center gap-2.5"
                >
                  <motion.span
                    initial={{ opacity: 0, x: 8 }}
                    animate={{ opacity: 1, x: 0 }}
                    exit={{ opacity: 0, x: 8 }}
                    transition={{ delay: i * 0.05 + 0.06 }}
                    className="rounded-lg border border-dark-border bg-dark-surface2/90 backdrop-blur-sm px-3 py-1.5 text-xs font-medium text-dark-text shadow-lg whitespace-nowrap"
                  >
                    {t(item.labelKey)}
                  </motion.span>

                  {/* Icon button */}
                  <a
                    href={item.href}
                    target={item.external ? "_blank" : undefined}
                    rel={item.external ? "noopener noreferrer" : undefined}
                    onClick={(event) => {
                      if (item.id === "langbot") {
                        event.preventDefault();
                        document
                          .querySelector<HTMLElement>("#langbot-widget-root")
                          ?.shadowRoot?.querySelector<HTMLButtonElement>(".lb-bubble")
                          ?.click();
                      }
                      setOpen(false);
                    }}
                    className="flex items-center justify-center w-11 h-11 rounded-full transition-transform hover:scale-110 active:scale-95 cursor-pointer shadow-lg"
                    style={{
                      background: item.color,
                      boxShadow: `0 4px 16px ${item.shadowColor}`,
                    }}
                    onMouseEnter={(e) => {
                      (e.currentTarget as HTMLAnchorElement).style.background = item.hoverColor;
                    }}
                    onMouseLeave={(e) => {
                      (e.currentTarget as HTMLAnchorElement).style.background = item.color;
                    }}
                  >
                    {item.icon}
                  </a>
                </motion.div>
              ))}
          </AnimatePresence>

          {/* 主按钮 */}
          <button
            onClick={() => setOpen((v) => !v)}
            className="relative flex items-center justify-center w-13 h-13 rounded-full cursor-pointer transition-transform hover:scale-110 active:scale-95 shadow-xl"
            style={{
              width: 52,
              height: 52,
              background: "linear-gradient(135deg, #38bdf8 0%, #3b82f6 100%)",
              boxShadow: "0 6px 24px rgba(59,130,246,0.40)",
            }}
            aria-label={t("community.floatLabel")}
          >
            <motion.span
              animate={{ rotate: open ? 45 : 0 }}
              transition={{ type: "spring", stiffness: 400, damping: 25 }}
              className="flex items-center justify-center"
            >
              {open ? (
                /* × close icon */
                <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5" strokeLinecap="round">
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              ) : (
                /* community / chat bubble icon */
                <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                  <line x1="9" y1="10" x2="9" y2="10" strokeWidth="2.5" />
                  <line x1="12" y1="10" x2="12" y2="10" strokeWidth="2.5" />
                  <line x1="15" y1="10" x2="15" y2="10" strokeWidth="2.5" />
                </svg>
              )}
            </motion.span>

            {/* 未展开时的呼吸光晕 */}
            {!open && (
              <span
                className="absolute inset-0 rounded-full animate-ping opacity-20"
                style={{ background: "#38bdf8" }}
              />
            )}
          </button>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
