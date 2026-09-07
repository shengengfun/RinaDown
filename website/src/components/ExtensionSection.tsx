import { motion, AnimatePresence } from "framer-motion";
import { MousePointerClick, Radio, Filter, Moon, Inbox, Settings, Video, Image as ImageIcon, Music, Download } from "lucide-react";
import { DotBackground } from "@/components/ui/grid-background";
import { useState } from "react";
import { useLocale } from "@/lib/i18n";

/* ============================================================
   ExtensionSection — Browser Extension Popup Mockup
   Mirrors the real extension UI: header (lang/theme/status),
   tabs (tasks / resources / settings), empty task card with a
   paste bar, today stats, and a footer.
   ============================================================ */

type ExtTab = "tasks" | "resources" | "settings";

export default function ExtensionSection() {
  const [popupVisible, setPopupVisible] = useState(true);
  const [activeTab, setActiveTab] = useState<ExtTab>("tasks");
  const [pasteUrl, setPasteUrl] = useState("");
  const { t } = useLocale();

  return (
    <section id="extension" className="relative py-20 sm:py-32 overflow-hidden">
      <DotBackground className="absolute inset-0 -z-10" />

      <div className="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8 relative z-10">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">
          {/* Left: Content */}
          <motion.div
            initial={{ opacity: 0, x: -40 }}
            whileInView={{ opacity: 1, x: 0 }}
            viewport={{ once: true, margin: "-100px" }}
            transition={{ duration: 0.7 }}
            className="space-y-6"
          >
            <div>
              <span className="inline-flex items-center px-3 py-1 rounded-full text-xs font-semibold bg-[#06b6d4]/10 text-[#06b6d4] border border-[#06b6d4]/20 uppercase tracking-widest mb-4">
                {t("ext.badge")}
              </span>
              <h2 className="text-3xl sm:text-4xl lg:text-5xl font-bold tracking-tight text-dark-text">
                {t("ext.title")}
                <span className="bg-gradient-to-r from-[#06b6d4] to-[#38bdf8] bg-clip-text text-transparent">
                  {t("ext.titleHighlight")}
                </span>
              </h2>
              <p className="mt-4 text-dark-text-secondary text-lg leading-relaxed">
                {t("ext.subtitle")}
              </p>
            </div>

            <div className="space-y-4">
              {[
                {
                  Icon: MousePointerClick,
                  iconBoxClass: "bg-sky-500/10 border-sky-500/20",
                  iconClass: "text-sky-400",
                  titleKey: "ext.feat1.title" as const,
                  descKey: "ext.feat1.desc" as const,
                },
                {
                  Icon: Radio,
                  iconBoxClass: "bg-emerald-500/10 border-emerald-500/20",
                  iconClass: "text-emerald-400",
                  titleKey: "ext.feat2.title" as const,
                  descKey: "ext.feat2.desc" as const,
                },
                {
                  Icon: Filter,
                  iconBoxClass: "bg-violet-500/10 border-violet-500/20",
                  iconClass: "text-violet-400",
                  titleKey: "ext.feat3.title" as const,
                  descKey: "ext.feat3.desc" as const,
                },
              ].map((item, i) => (
                <motion.div
                  key={item.titleKey}
                  initial={{ opacity: 0, y: 20 }}
                  whileInView={{ opacity: 1, y: 0 }}
                  viewport={{ once: true }}
                  transition={{ delay: 0.2 + i * 0.1, duration: 0.5 }}
                  className="flex gap-4"
                >
                  <div
                    className={`shrink-0 w-10 h-10 rounded-lg border flex items-center justify-center ${item.iconBoxClass}`}
                  >
                    <item.Icon className={`w-5 h-5 ${item.iconClass}`} />
                  </div>
                  <div>
                    <h4 className="text-sm font-semibold text-dark-text">
                      {t(item.titleKey)}
                    </h4>
                    <p className="text-xs text-dark-text-secondary mt-0.5">
                      {t(item.descKey)}
                    </p>
                  </div>
                </motion.div>
              ))}
            </div>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true }}
              transition={{ delay: 0.5, duration: 0.5 }}
              className="flex flex-wrap gap-3 pt-2"
            >
              <a
                href="https://chromewebstore.google.com/detail/fluxdown/meleenglfggcmcajknpeeeiobnpfmahc"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 rounded-lg bg-[#06b6d4]/10 border border-[#06b6d4]/30 px-5 py-2.5 text-sm font-semibold text-[#06b6d4] hover:bg-[#06b6d4]/20 transition-colors"
              >
                <svg
                  className="w-4 h-4"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                  aria-hidden="true"
                >
                  <path d="M12 0C8.21 0 4.831 1.757 2.632 4.501l3.953 6.848A5.454 5.454 0 0 1 12 6.545h10.691A12 12 0 0 0 12 0zM1.931 5.47A11.943 11.943 0 0 0 0 12c0 6.012 4.42 10.991 10.189 11.864l3.953-6.847a5.45 5.45 0 0 1-6.865-2.29zm13.342 2.166a5.446 5.446 0 0 1 1.45 7.09l.002.001h-.002l-5.344 9.257c.206.01.413.016.621.016 6.627 0 12-5.373 12-12 0-1.54-.29-3.011-.818-4.364zM12 16.364a4.364 4.364 0 1 1 0-8.728 4.364 4.364 0 0 1 0 8.728z" />
                </svg>
                {t("ext.addToChrome")}
              </a>
              <a
                href="https://microsoftedge.microsoft.com/addons/detail/fluxdown/nglkkjbogjghekbhhcnccnpfedjbdhhd"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 rounded-lg bg-[#0078d4]/10 border border-[#0078d4]/30 px-5 py-2.5 text-sm font-semibold text-[#3b9eff] hover:bg-[#0078d4]/20 transition-colors"
              >
                <svg
                  className="w-4 h-4"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                  aria-hidden="true"
                >
                  <path d="M21.86 17.86q.14 0 .25.12.1.13.1.25t-.11.33l-.32.46-.43.53-.44.5q-.21.25-.38.42l-.22.22q-.78.74-1.7 1.36-.91.62-1.92 1.07-1 .44-2.07.69-1.06.25-2.13.25-1.41 0-2.74-.36-1.34-.36-2.51-1Q6.16 22 5.21 21.07q-.95-.94-1.6-2.13-.56-1.04-.84-2.18-.27-1.13-.27-2.31 0-1.4.4-2.7.4-1.31 1.16-2.43.78-1.12 1.86-2.01 1.1-.89 2.46-1.46.64-.27 1.26-.39.62-.13 1.25-.13.95 0 1.85.27.91.27 1.69.78.78.51 1.4 1.23.63.72 1.05 1.61.42.89.65 1.91.23 1 .23 2.1 0 1.2-.32 2.21-.31 1.02-.86 1.85-.54.83-1.27 1.45-.72.61-1.55 1-.82.4-1.69.59-.87.18-1.72.18-.61 0-1.23-.1-.61-.08-1.18-.27-.58-.18-1.1-.46-.53-.27-.99-.65l.49.06.51.02q.94 0 1.84-.25.89-.24 1.69-.69.8-.45 1.45-1.09.66-.65 1.13-1.45.84-1.43.84-3.16 0-1.62-.83-2.95-.81-1.32-2.16-2.04-.71-.37-1.49-.56-.78-.18-1.59-.18-1.42 0-2.71.55-1.27.55-2.31 1.49-1.04.94-1.72 2.21-.69 1.27-.85 2.7l-.04.5-.01.51v.51l.04.5q.05.45.13.91.09.45.23.88.13.42.31.83.18.4.41.78.54.82 1.21 1.5.66.69 1.43 1.21.78.53 1.65.89.87.36 1.78.55 1.04.16 2.09.16 1.06 0 2.09-.27 1.04-.27 1.99-.79.96-.51 1.81-1.27.84-.74 1.55-1.74.05-.07.16-.16.11-.08.22-.13.07-.04.13-.04zM7.66 15.41q-.05-.34-.06-.66-.02-.34-.02-.62 0-.78.16-1.43.17-.66.43-1.21.27-.55.61-1 .35-.45.67-.81-.92.43-1.69 1.04-.78.61-1.34 1.34-.55.74-.86 1.59-.31.84-.31 1.74 0 .26.04.55.04.29.1.59.07.29.16.59.1.3.21.58.38 1.01 1.06 1.84.69.84 1.6 1.45.91.61 2 .94 1.11.34 2.31.34 1.16 0 2.32-.32 1.18-.32 2.18-.94 1-.62 1.78-1.51.78-.89 1.21-1.99-.69.59-1.4 1.06-.71.46-1.49.79-.78.32-1.59.5-.83.18-1.7.18-1.39 0-2.55-.41-1.16-.4-2.05-1.13-.89-.74-1.49-1.74-.6-1-.84-2.21-.05-.27-.09-.55-.05-.27-.06-.55l-.01-.05-.51-.05z" />
                </svg>
                {t("ext.addToEdge")}
              </a>
              <a
                href="https://addons.mozilla.org/firefox/addon/fluxdown/"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 rounded-lg bg-[#ff7139]/10 border border-[#ff7139]/30 px-5 py-2.5 text-sm font-semibold text-[#ff7139] hover:bg-[#ff7139]/20 transition-colors"
              >
                <svg
                  className="w-4 h-4"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                  aria-hidden="true"
                >
                  <path d="M20.452 8.427c-.386-.928-1.17-1.929-1.784-2.246.5.98.79 1.963.9 2.696l.002.014c-1.005-2.506-2.71-3.516-4.103-5.716a10.62 10.62 0 0 1-.202-.328 2.66 2.66 0 0 1-.095-.176 1.564 1.564 0 0 1-.128-.34.022.022 0 0 0-.02-.022.03.03 0 0 0-.016 0l-.004.001-.007.004.004-.007c-2.234 1.309-2.991 3.732-3.06 4.944a4.446 4.446 0 0 0-2.446.943 2.628 2.628 0 0 0-.228-.172 4.11 4.11 0 0 1-.025-2.167c-.911.415-1.62 1.072-2.135 1.653h-.004c-.352-.446-.327-1.918-.307-2.226a1.588 1.588 0 0 0-.297.158 6.454 6.454 0 0 0-.867.743 7.756 7.756 0 0 0-.828 1l-.005.007.004-.006a7.482 7.482 0 0 0-1.19 2.685l-.012.058c-.017.078-.078.474-.088.56 0 .007-.002.013-.002.02a8.505 8.505 0 0 0-.145 1.226v.046a8.898 8.898 0 0 0 17.685 1.482c.015-.114.027-.227.04-.343a9.147 9.147 0 0 0-.567-4.194zm-11.9 6.803c.042.02.08.042.124.061l.006.004a5.06 5.06 0 0 1-.13-.065zm11.351-5.61v-.011l.002.012z" />
                </svg>
                {t("ext.addToFirefox")}
              </a>
            </motion.div>
          </motion.div>

          {/* Right: Interactive Extension Popup Mockup */}
          <motion.div
            initial={{ opacity: 0, x: 40 }}
            whileInView={{ opacity: 1, x: 0 }}
            viewport={{ once: true, margin: "-100px" }}
            transition={{ duration: 0.7, delay: 0.1 }}
            className="relative"
          >
            <div className="absolute -inset-8 rounded-3xl bg-gradient-to-br from-[#06b6d4]/10 via-transparent to-brand-blue/10 blur-2xl opacity-50 pointer-events-none" />

            <div className="relative rounded-xl border border-dark-border bg-dark-surface2 shadow-2xl overflow-hidden max-w-sm mx-auto select-none">
              {/* Browser toolbar */}
              <div className="flex items-center gap-2 px-3 py-2.5 border-b border-dark-border bg-dark-surface1">
                <div className="flex gap-1.5">
                  <div className="w-2.5 h-2.5 rounded-full bg-danger/60 hover:bg-danger transition-colors cursor-pointer" />
                  <div className="w-2.5 h-2.5 rounded-full bg-warning/60 hover:bg-warning transition-colors cursor-pointer" />
                  <div className="w-2.5 h-2.5 rounded-full bg-success/60 hover:bg-success transition-colors cursor-pointer" />
                </div>
                <div className="flex-1 mx-2 bg-dark-bg rounded-md px-3 py-1">
                  <span className="text-[10px] text-dark-text-muted">
                    example.com/downloads
                  </span>
                </div>
                {/* Extension icon — clickable */}
                <motion.div
                  onClick={() => setPopupVisible((v) => !v)}
                  className="w-6 h-6 rounded flex items-center justify-center cursor-pointer"
                  whileTap={{ scale: 0.9 }}
                  animate={{
                    backgroundColor: popupVisible
                      ? "rgba(59,130,246,0.3)"
                      : "rgba(59,130,246,0.1)",
                  }}
                  transition={{ duration: 0.15 }}
                >
                  <img src="/logo.svg" alt="" className="w-4 h-4" />
                </motion.div>
              </div>

              {/* Popup content — toggled by clicking extension icon */}
              <AnimatePresence>
                {popupVisible && (
                  <motion.div
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: "auto", opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ duration: 0.25, ease: "easeInOut" }}
                    className="overflow-hidden"
                  >
                    <div className="bg-dark-surface1 p-4 space-y-3.5">
                      {/* Header: logo + lang/theme/status */}
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          <img src="/logo.svg" alt="" className="w-6 h-6" />
                          <span className="text-sm font-semibold">
                            <span className="text-brand-sky">Flux</span>
                            <span className="text-dark-text">Down</span>
                          </span>
                        </div>
                        <div className="flex items-center gap-1.5">
                          <div className="flex items-center justify-center h-6 px-1.5 rounded-md border border-dark-border bg-dark-surface2 text-[10px] font-medium text-dark-text-secondary cursor-default">
                            中
                          </div>
                          <div className="flex items-center justify-center w-6 h-6 rounded-md border border-dark-border bg-dark-surface2 text-dark-text-secondary cursor-default">
                            <Moon className="w-3 h-3" />
                          </div>
                          <div className="flex items-center gap-1 h-6 px-2 rounded-md border border-success/25 bg-success/10 cursor-default">
                            <motion.div
                              className="w-1.5 h-1.5 rounded-full bg-success"
                              animate={{ scale: [1, 1.3, 1] }}
                              transition={{ repeat: Infinity, duration: 2 }}
                            />
                            <span className="text-[10px] font-medium text-success">
                              {t("ext.connected")}
                            </span>
                          </div>
                        </div>
                      </div>

                      {/* Tabs */}
                      <div className="flex items-center gap-5 border-b border-dark-border">
                        {[
                          { key: "tasks" as const, label: t("ext.tabTasks"), badge: 0 },
                          { key: "resources" as const, label: t("ext.tabResources"), badge: 30 },
                          { key: "settings" as const, label: t("ext.tabSettings"), badge: 0 },
                        ].map((tab) => (
                          <button
                            key={tab.key}
                            onClick={() => setActiveTab(tab.key)}
                            className="relative flex items-center gap-1.5 pb-2 text-xs font-medium transition-colors"
                            style={{
                              color:
                                activeTab === tab.key
                                  ? "#38bdf8"
                                  : "var(--color-dark-text-muted)",
                            }}
                          >
                            {tab.label}
                            {tab.badge > 0 && (
                              <span className="inline-flex items-center justify-center min-w-4 h-4 px-1 rounded-full bg-brand-sky text-[9px] font-bold text-white">
                                {tab.badge}
                              </span>
                            )}
                            {activeTab === tab.key && (
                              <motion.div
                                layoutId="ext-tab-underline"
                                className="absolute -bottom-px left-0 right-0 h-0.5 rounded-full bg-brand-sky"
                              />
                            )}
                          </button>
                        ))}
                      </div>

                      {/* Pane content — switches with the active tab */}
                      <div className="min-h-[188px]">
                        {/* Tasks pane */}
                        {activeTab === "tasks" && (
                          <div className="space-y-3">
                            <div className="rounded-lg border border-dark-border bg-dark-surface2 p-4 space-y-3">
                              <div className="flex flex-col items-center justify-center py-4 text-center">
                                <Inbox className="w-9 h-9 text-dark-text-muted/60" />
                                <p className="mt-2 text-xs font-medium text-dark-text-secondary">
                                  {t("ext.emptyTasks")}
                                </p>
                              </div>
                              <div className="flex items-center gap-2">
                                <input
                                  type="text"
                                  value={pasteUrl}
                                  onChange={(e) => setPasteUrl(e.target.value)}
                                  placeholder={t("ext.pastePlaceholder")}
                                  className="flex-1 min-w-0 h-8 rounded-md border border-dark-border bg-dark-bg px-2.5 text-[11px] text-dark-text placeholder:text-dark-text-muted outline-none focus:border-brand-sky/50 transition-colors"
                                />
                                <button className="shrink-0 h-8 px-3 rounded-md bg-brand-sky text-[11px] font-semibold text-white hover:bg-brand-sky/90 transition-colors">
                                  {t("ext.pasteButton")}
                                </button>
                              </div>
                            </div>
                            {/* Today stats */}
                            <div className="flex items-center justify-center gap-1.5 text-[11px]">
                              <span className="text-dark-text-muted">{t("ext.todayLabel")}</span>
                              <span className="text-success font-semibold">2</span>
                              <span className="text-dark-text-secondary">{t("ext.takenOver")}</span>
                              <span className="text-dark-text-muted">·</span>
                              <span className="text-danger font-semibold">0</span>
                              <span className="text-dark-text-secondary">{t("ext.failed")}</span>
                            </div>
                          </div>
                        )}

                        {/* Resources pane */}
                        {activeTab === "resources" && (
                          <div className="space-y-2.5">
                            <div className="flex flex-wrap gap-1.5">
                              {[
                                { label: t("ext.resTypeAll"), active: true },
                                { label: t("ext.resTypeVideo"), active: false },
                                { label: t("ext.resTypeImage"), active: false },
                                { label: t("ext.resTypeAudio"), active: false },
                              ].map((rt) => (
                                <span
                                  key={rt.label}
                                  className={`px-2 py-0.5 text-[10px] rounded-full border ${
                                    rt.active
                                      ? "bg-brand-sky/12 border-brand-sky/35 text-brand-sky"
                                      : "bg-dark-surface2 border-dark-border text-dark-text-muted"
                                  }`}
                                >
                                  {rt.label}
                                </span>
                              ))}
                            </div>
                            <div className="space-y-1.5">
                              {[
                                { Icon: Video, name: "trailer-4k.mp4", meta: "MP4 · 248 MB", color: "text-[#EC4899]" },
                                { Icon: Music, name: "podcast-ep42.mp3", meta: "MP3 · 36 MB", color: "text-[#22C55E]" },
                                { Icon: ImageIcon, name: "cover-art.png", meta: "PNG · 4.2 MB", color: "text-brand-sky" },
                              ].map((r) => (
                                <div
                                  key={r.name}
                                  className="flex items-center gap-2 rounded-md border border-dark-border bg-dark-surface2 px-2 py-1.5"
                                >
                                  <r.Icon className={`w-4 h-4 shrink-0 ${r.color}`} />
                                  <div className="min-w-0 flex-1">
                                    <div className="truncate text-[11px] text-dark-text">{r.name}</div>
                                    <div className="text-[9px] text-dark-text-muted">{r.meta}</div>
                                  </div>
                                  <Download className="w-3.5 h-3.5 shrink-0 text-dark-text-muted" />
                                </div>
                              ))}
                            </div>
                            <div className="flex items-center justify-between pt-0.5">
                              <label className="flex items-center gap-1.5 text-[10px] text-dark-text-muted">
                                <span className="w-3 h-3 rounded-sm border border-dark-border bg-dark-surface2" />
                                {t("ext.selectAll")}
                              </label>
                              <button className="h-6 px-2.5 rounded-md bg-brand-sky/12 border border-brand-sky/30 text-[10px] font-semibold text-brand-sky">
                                {t("ext.batchDownload")} (0)
                              </button>
                            </div>
                          </div>
                        )}

                        {/* Settings pane */}
                        {activeTab === "settings" && (
                          <div className="space-y-2.5">
                            {[
                              { label: t("ext.setIntercept"), hint: t("ext.setEnabled"), on: true },
                              { label: t("ext.setFloatingBall"), hint: "", on: true },
                              { label: t("ext.setSniffing"), hint: "", on: true },
                            ].map((s) => (
                              <div
                                key={s.label}
                                className="flex items-center justify-between rounded-md border border-dark-border bg-dark-surface2 px-3 py-2"
                              >
                                <div className="flex flex-col">
                                  <span className="text-[11px] font-medium text-dark-text">{s.label}</span>
                                  {s.hint && (
                                    <span className="text-[9px] text-success">{s.hint}</span>
                                  )}
                                </div>
                                <div
                                  className="relative w-8 h-[18px] rounded-full shrink-0"
                                  style={{ backgroundColor: s.on ? "#22C55E" : "var(--color-dark-text-muted)" }}
                                >
                                  <span
                                    className="absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white shadow-sm"
                                    style={{ left: s.on ? 16 : 2 }}
                                  />
                                </div>
                              </div>
                            ))}
                            <div className="text-[9px] text-dark-text-muted leading-relaxed px-0.5">
                              {t("ext.setSniffingHint")}
                            </div>
                            <div className="flex items-center justify-between rounded-md border border-dark-border bg-dark-surface2 px-3 py-2">
                              <span className="text-[11px] font-medium text-dark-text">{t("ext.setRemoteMode")}</span>
                              <span className="text-[10px] text-dark-text-secondary rounded border border-dark-border bg-dark-bg px-1.5 py-0.5">
                                {t("ext.remoteFallback")}
                              </span>
                            </div>
                          </div>
                        )}
                      </div>

                      {/* Footer: all settings + dev */}
                      <div className="flex items-center justify-between pt-2.5 border-t border-dark-border">
                        <div className="flex items-center gap-1.5 text-[11px] text-brand-sky cursor-default">
                          <Settings className="w-3 h-3" />
                          <span>{t("ext.allSettings")}</span>
                        </div>
                        <span className="text-[10px] text-dark-text-muted">dev</span>
                      </div>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          </motion.div>
        </div>
      </div>
    </section>
  );
}
