import { motion } from "framer-motion";
import { ArrowLeft, Terminal, Shield, AlertTriangle, CheckCircle2, Copy } from "lucide-react";
import { useLocale } from "@/lib/i18n";
import { useState } from "react";

function CodeBlock({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <div className="relative group my-3 rounded-lg bg-dark-surface2 border border-dark-border overflow-hidden">
      <div className="flex items-center justify-between px-4 py-2 border-b border-dark-border/60 bg-dark-surface1/50">
        <div className="flex items-center gap-2">
          <Terminal className="w-3.5 h-3.5 text-dark-text-muted" />
          <span className="text-[10px] font-medium text-dark-text-muted uppercase tracking-widest">
            终端
          </span>
        </div>
        <button
          type="button"
          onClick={handleCopy}
          className="flex items-center gap-1.5 px-2.5 py-1 rounded text-[10px] font-medium text-dark-text-muted hover:text-dark-text-secondary hover:bg-dark-surface3 transition-colors"
        >
          {copied ? (
            <>
              <CheckCircle2 className="w-3 h-3 text-success" />
              <span className="text-success">已复制</span>
            </>
          ) : (
            <>
              <Copy className="w-3 h-3" />
              复制
            </>
          )}
        </button>
      </div>
      <pre className="px-4 py-3 text-xs text-brand-sky font-mono overflow-x-auto whitespace-pre-wrap break-all leading-relaxed">
        {code}
      </pre>
    </div>
  );
}

function StepBadge({ n }: { n: number }) {
  return (
    <span className="inline-flex items-center justify-center w-5 h-5 rounded-full bg-brand-blue/20 text-brand-blue border border-brand-blue/30 text-[10px] font-bold flex-shrink-0 mt-0.5">
      {n}
    </span>
  );
}

export default function MacosGatekeeperPage() {
  const { t } = useLocale();

  return (
    <section className="relative py-20 sm:py-28 overflow-hidden bg-dark-bg">
      {/* Background ambiance */}
      <div className="absolute inset-0 -z-10 overflow-hidden pointer-events-none">
        <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[600px] h-[300px] bg-brand-sky/[0.03] blur-[120px] rounded-full" />
        <div className="absolute bottom-1/4 right-0 w-[400px] h-[400px] bg-brand-cyan/[0.02] blur-[100px] rounded-full" />
      </div>

      <div className="mx-auto max-w-3xl px-4 sm:px-6 lg:px-8 relative z-10">
        {/* Back link */}
        <motion.div
          initial={{ opacity: 0, x: -10 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 0.3 }}
          className="mb-8"
        >
          <a
            href="/#download"
            className="inline-flex items-center gap-2 text-sm text-dark-text-muted hover:text-brand-sky transition-colors"
          >
            <ArrowLeft className="w-4 h-4" />
            {t("macos.backToDownload")}
          </a>
        </motion.div>

        {/* Header */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.4 }}
          className="text-center mb-12"
        >
          <div className="inline-flex items-center justify-center w-14 h-14 rounded-xl bg-gradient-to-br from-yellow-500/20 to-orange-500/20 border border-yellow-500/20 mb-5">
            <Shield className="w-7 h-7 text-yellow-400" />
          </div>
          <h1 className="text-3xl sm:text-4xl font-bold tracking-tight text-dark-text">
            {t("macos.title")}
          </h1>
          <p className="mt-4 text-dark-text-secondary text-sm leading-relaxed max-w-2xl mx-auto">
            {t("macos.subtitle")}
          </p>
          <p className="mt-2 text-dark-text-muted text-xs">{t("macos.lastUpdated")}</p>
        </motion.div>

        <div className="space-y-6">
          {/* Why section */}
          <motion.div
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.4, delay: 0.05 }}
            className="rounded-xl border border-yellow-500/20 bg-yellow-500/[0.05] p-5"
          >
            <div className="flex items-start gap-3">
              <AlertTriangle className="w-4 h-4 text-yellow-400 flex-shrink-0 mt-0.5" />
              <div>
                <h2 className="text-sm font-semibold text-dark-text mb-2">
                  {t("macos.whyTitle")}
                </h2>
                <p className="text-xs text-dark-text-secondary leading-relaxed">
                  {t("macos.whyDesc")}
                </p>
              </div>
            </div>
          </motion.div>

          {/* Method 1 */}
          <motion.div
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.4, delay: 0.1 }}
            className="rounded-xl border border-dark-border bg-dark-surface1 p-5 sm:p-6"
          >
            <div className="flex items-center gap-2 mb-4">
              <div className="w-7 h-7 rounded-lg bg-brand-blue/20 border border-brand-blue/30 flex items-center justify-center flex-shrink-0">
                <span className="text-xs font-bold text-brand-blue">1</span>
              </div>
              <h2 className="text-sm font-semibold text-dark-text">
                {t("macos.method1Title")}
              </h2>
            </div>
            <ol className="space-y-4">
              <li className="flex items-start gap-3">
                <StepBadge n={1} />
                <p className="text-xs text-dark-text-secondary leading-relaxed">
                  {t("macos.method1Step1")}
                </p>
              </li>
              <li className="flex flex-col gap-1">
                <div className="flex items-start gap-3">
                  <StepBadge n={2} />
                  <p className="text-xs text-dark-text-secondary leading-relaxed">
                    {t("macos.method1Step2")}
                  </p>
                </div>
                <CodeBlock code="sudo spctl --master-disable" />
              </li>
              <li className="flex items-start gap-3">
                <StepBadge n={3} />
                <p className="text-xs text-dark-text-secondary leading-relaxed">
                  {t("macos.method1Step3")}
                </p>
              </li>
            </ol>
          </motion.div>

          {/* Method 2 */}
          <motion.div
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.4, delay: 0.15 }}
            className="rounded-xl border border-dark-border bg-dark-surface1 p-5 sm:p-6"
          >
            <div className="flex items-center gap-2 mb-4">
              <div className="w-7 h-7 rounded-lg bg-brand-sky/20 border border-brand-sky/30 flex items-center justify-center flex-shrink-0">
                <span className="text-xs font-bold text-brand-sky">2</span>
              </div>
              <h2 className="text-sm font-semibold text-dark-text">
                {t("macos.method2Title")}
              </h2>
            </div>
            <p className="text-xs text-dark-text-secondary leading-relaxed mb-1">
              {t("macos.method2Desc")}
            </p>
            <CodeBlock code="sudo xattr -rd com.apple.quarantine /Applications/FluxDown.app" />
            <p className="text-[10px] text-dark-text-muted leading-relaxed mt-2 pl-1 border-l-2 border-dark-border">
              {t("macos.method2Note")}
            </p>
          </motion.div>

          {/* Method 3 */}
          <motion.div
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.4, delay: 0.2 }}
            className="rounded-xl border border-dark-border bg-dark-surface1 p-5 sm:p-6"
          >
            <div className="flex items-center gap-2 mb-4">
              <div className="w-7 h-7 rounded-lg bg-brand-cyan/20 border border-brand-cyan/30 flex items-center justify-center flex-shrink-0">
                <span className="text-xs font-bold text-brand-cyan">3</span>
              </div>
              <h2 className="text-sm font-semibold text-dark-text">
                {t("macos.method3Title")}
              </h2>
            </div>
            <ol className="space-y-4">
              <li className="flex flex-col gap-1">
                <div className="flex items-start gap-3">
                  <StepBadge n={1} />
                  <p className="text-xs text-dark-text-secondary leading-relaxed">
                    {t("macos.method3Step1")}
                  </p>
                </div>
                <CodeBlock code="xcode-select --install" />
              </li>
              <li className="flex flex-col gap-1">
                <div className="flex items-start gap-3">
                  <StepBadge n={2} />
                  <p className="text-xs text-dark-text-secondary leading-relaxed">
                    {t("macos.method3Step2")}
                  </p>
                </div>
                <CodeBlock code="sudo codesign --force --deep --sign - /Applications/FluxDown.app" />
                <p className="text-[10px] text-dark-text-muted leading-relaxed mt-1 pl-1 border-l-2 border-dark-border">
                  {t("macos.method3Note")}
                </p>
              </li>
            </ol>
          </motion.div>

          {/* Tip box */}
          <motion.div
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.4, delay: 0.25 }}
            className="rounded-xl border border-brand-blue/20 bg-brand-blue/[0.05] p-5"
          >
            <h3 className="text-sm font-semibold text-dark-text mb-2">
              {t("macos.tipTitle")}
            </h3>
            <p className="text-xs text-dark-text-secondary leading-relaxed">
              {t("macos.tipDesc")}
            </p>
          </motion.div>

          {/* Back button */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.4, delay: 0.3 }}
            className="flex justify-center pt-4 pb-8"
          >
            <a
              href="/#download"
              className="inline-flex items-center gap-2 rounded-lg bg-brand-blue px-6 py-2.5 text-sm font-semibold text-white hover:bg-brand-blue/90 transition-colors shadow-lg shadow-brand-blue/20"
            >
              <ArrowLeft className="w-4 h-4" />
              {t("macos.backToDownload")}
            </a>
          </motion.div>
        </div>
      </div>
    </section>
  );
}
