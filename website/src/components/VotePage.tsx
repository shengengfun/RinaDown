import { useState, useEffect, useCallback } from "react";
import { motion } from "framer-motion";
import { useLocale } from "@/lib/i18n";
import type { Messages } from "@/lib/locales";

type VoteOption = "wechat" | "qq" | "official-account";

interface VoteResults {
  results: Record<string, number>;
  total: number;
}

const STORAGE_KEY = "fluxdown-voted-community";

const OPTIONS: {
  key: VoteOption;
  nameKey: keyof Messages;
  descKey: keyof Messages;
  icon: React.ReactNode;
  gradient: string;
  accent: string;
}[] = [
  {
    key: "wechat",
    nameKey: "vote.wechat",
    descKey: "vote.wechatDesc",
    gradient: "from-[#07C160]/20 to-[#07C160]/5",
    accent: "#07C160",
    icon: (
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M8.5 14c-3.033 0-5.5-2.015-5.5-4.5S5.467 5 8.5 5s5.5 2.015 5.5 4.5c0 .892-.304 1.726-.832 2.432L14 14.5l-2.658-.665A6.483 6.483 0 0 1 8.5 14Z" />
        <path d="M15.5 19c-1.303 0-2.505-.37-3.457-.993L9 18.75l.562-2.18A4.728 4.728 0 0 1 8.5 14c.553.05 1.078.05 1.5 0 .7 1.69 2.714 3 5 3 .83 0 1.62-.153 2.342-.432L20 17.5l-.832-2.568c.528-.706.832-1.54.832-2.432 0-.622-.144-1.214-.4-1.75" />
        <circle cx="7" cy="9.5" r="0.5" fill="currentColor" stroke="none" />
        <circle cx="10" cy="9.5" r="0.5" fill="currentColor" stroke="none" />
      </svg>
    ),
  },
  {
    key: "qq",
    nameKey: "vote.qq",
    descKey: "vote.qqDesc",
    gradient: "from-[#12B7F5]/20 to-[#12B7F5]/5",
    accent: "#12B7F5",
    icon: (
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5Z" />
        <circle cx="9.5" cy="11" r="0.5" fill="currentColor" stroke="none" />
        <circle cx="14.5" cy="11" r="0.5" fill="currentColor" stroke="none" />
        <path d="M8 14s1.5 2 4 2 4-2 4-2" />
      </svg>
    ),
  },
  {
    key: "official-account",
    nameKey: "vote.officialAccount",
    descKey: "vote.officialAccountDesc",
    gradient: "from-[#FA5151]/20 to-[#FA5151]/5",
    accent: "#FA5151",
    icon: (
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="m3 11 18-5v12L3 13v-2Z" />
        <path d="M11.6 16.8a3 3 0 1 1-5.8-1.6" />
      </svg>
    ),
  },
];

export default function VotePage() {
  const { t } = useLocale();
  const [results, setResults] = useState<VoteResults | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [votedOption, setVotedOption] = useState<VoteOption | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [statusMsg, setStatusMsg] = useState<{ text: string; type: "success" | "error" } | null>(null);

  useEffect(() => {
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved) setVotedOption(saved as VoteOption);
    } catch {
      // localStorage unavailable
    }

    fetch("/api/vote")
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then((data: VoteResults) => setResults(data))
      .catch(() => setLoadError(true))
      .finally(() => setLoading(false));
  }, []);

  const handleVote = useCallback(async (option: VoteOption) => {
    if (votedOption || submitting) return;

    setSubmitting(true);
    setStatusMsg(null);

    try {
      const res = await fetch("/api/vote", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ option }),
      });

      if (res.status === 429) {
        setStatusMsg({ text: t("vote.rateLimited"), type: "error" });
        return;
      }

      if (!res.ok) {
        setStatusMsg({ text: t("vote.error"), type: "error" });
        return;
      }

      const data = await res.json();

      if (data.message === "already_voted") {
        setVotedOption(option);
        setStatusMsg({ text: t("vote.alreadyVoted"), type: "success" });
      } else {
        setVotedOption(option);
        setStatusMsg({ text: t("vote.success"), type: "success" });
        setResults((prev) => {
          if (!prev) return prev;
          const updated = { ...prev.results };
          updated[option] = (updated[option] || 0) + 1;
          return { results: updated, total: prev.total + 1 };
        });
      }

      try {
        localStorage.setItem(STORAGE_KEY, option);
      } catch {
        // localStorage unavailable
      }
    } catch {
      setStatusMsg({ text: t("vote.error"), type: "error" });
    } finally {
      setSubmitting(false);
    }
  }, [votedOption, submitting, t]);

  const getPercentage = (option: VoteOption): number => {
    if (!results || results.total === 0) return 0;
    return Math.round(((results.results[option] || 0) / results.total) * 100);
  };

  return (
    <section className="pt-24 sm:pt-32 pb-16 sm:pb-20">
      <div className="mx-auto max-w-4xl px-4 sm:px-6 lg:px-8">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="text-center mb-12 sm:mb-16"
        >
          <span className="inline-flex items-center gap-2 rounded-full border border-dark-border bg-dark-surface1/50 px-4 py-1.5 text-xs font-medium text-dark-text-secondary backdrop-blur-sm mb-6">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-brand-sky">
              <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
              <polyline points="22 4 12 14.01 9 11.01" />
            </svg>
            {t("vote.badge")}
          </span>

          <h1 className="text-4xl sm:text-5xl font-bold tracking-tight leading-tight">
            <span className="text-dark-text">{t("vote.title")}</span>
            <span className="bg-gradient-to-r from-brand-sky to-brand-cyan bg-clip-text text-transparent">{t("vote.titleHighlight")}</span>
          </h1>

          <p className="mt-4 text-base sm:text-lg text-dark-text-secondary max-w-2xl mx-auto leading-relaxed">
            {t("vote.subtitle")}
          </p>
        </motion.div>

        {loading && (
          <div className="flex items-center justify-center py-20">
            <div className="flex items-center gap-3 text-dark-text-muted">
              <svg className="w-5 h-5 animate-spin" viewBox="0 0 24 24" fill="none">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              <span className="text-sm">{t("vote.loading")}</span>
            </div>
          </div>
        )}

        {loadError && (
          <div className="flex items-center justify-center py-20">
            <span className="text-sm text-danger">{t("vote.loadError")}</span>
          </div>
        )}

        {!loading && !loadError && (
          <>
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 sm:gap-6">
              {OPTIONS.map((opt, i) => {
                const count = results?.results[opt.key] || 0;
                const pct = getPercentage(opt.key);
                const isVoted = votedOption === opt.key;
                const hasVoted = votedOption !== null;

                return (
                  <motion.div
                    key={opt.key}
                    initial={{ opacity: 0, y: 20 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.4, delay: 0.1 * i }}
                    onClick={() => handleVote(opt.key)}
                    className={`group relative rounded-xl border overflow-hidden transition-all duration-300 ${
                      hasVoted && !isVoted
                        ? "border-dark-border/50 opacity-60 cursor-default"
                        : hasVoted && isVoted
                          ? "border-brand-blue/50 cursor-default ring-1 ring-brand-blue/30"
                          : "border-dark-border hover:border-dark-text-muted cursor-pointer hover:shadow-lg hover:shadow-black/10 hover:-translate-y-1"
                    }`}
                  >
                    <div className={`absolute inset-0 bg-gradient-to-b ${opt.gradient} opacity-0 ${!hasVoted ? "group-hover:opacity-100" : ""} ${isVoted ? "!opacity-100" : ""} transition-opacity duration-300`} />

                    <div className="relative p-6">
                      <div className="flex items-center justify-between mb-4">
                        <div
                          className="flex items-center justify-center w-12 h-12 rounded-xl bg-dark-surface2 transition-colors"
                          style={{ color: isVoted ? opt.accent : undefined }}
                        >
                          {opt.icon}
                        </div>
                        {isVoted && (
                          <motion.div
                            initial={{ scale: 0 }}
                            animate={{ scale: 1 }}
                            transition={{ type: "spring", bounce: 0.5 }}
                          >
                            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke={opt.accent} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                              <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                              <polyline points="22 4 12 14.01 9 11.01" />
                            </svg>
                          </motion.div>
                        )}
                      </div>

                      <h3 className="text-lg font-semibold text-dark-text mb-1.5">
                        {t(opt.nameKey)}
                      </h3>
                      <p className="text-sm text-dark-text-muted leading-relaxed mb-6">
                        {t(opt.descKey)}
                      </p>

                      <div className="space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="text-sm font-medium text-dark-text-secondary tabular-nums">
                            {t("vote.votes", { n: String(count) })}
                          </span>
                          <span className="text-sm font-semibold tabular-nums" style={{ color: opt.accent }}>
                            {pct}%
                          </span>
                        </div>
                        <div className="h-2 rounded-full bg-dark-surface3 overflow-hidden">
                          <motion.div
                            className="h-full rounded-full"
                            style={{ backgroundColor: opt.accent }}
                            initial={{ width: 0 }}
                            animate={{ width: `${pct}%` }}
                            transition={{ duration: 0.8, delay: 0.2 + 0.1 * i, ease: [0.22, 1, 0.36, 1] }}
                          />
                        </div>
                      </div>

                      {!hasVoted && (
                        <div className="mt-4 flex items-center justify-center rounded-lg border border-dark-border bg-dark-surface1/50 py-2 text-sm font-medium text-dark-text-secondary group-hover:border-dark-text-muted group-hover:text-dark-text transition-colors">
                          {submitting ? t("vote.submitting") : t("vote.submitVote")}
                        </div>
                      )}
                    </div>
                  </motion.div>
                );
              })}
            </div>

            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ delay: 0.6 }}
              className="mt-8 text-center space-y-3"
            >
              <p className="text-sm text-dark-text-muted tabular-nums">
                {t("vote.totalVotes", { n: String(results?.total || 0) })}
              </p>

              {statusMsg && (
                <motion.p
                  initial={{ opacity: 0, y: 5 }}
                  animate={{ opacity: 1, y: 0 }}
                  className={`text-sm font-medium ${statusMsg.type === "success" ? "text-success" : "text-danger"}`}
                >
                  {statusMsg.text}
                </motion.p>
              )}
            </motion.div>
          </>
        )}
      </div>
    </section>
  );
}
