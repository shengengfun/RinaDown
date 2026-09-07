import { useState, useEffect, useCallback } from "react";
import { motion } from "framer-motion";
import { useLocale } from "@/lib/i18n";
import type { Messages } from "@/lib/locales";

type Plan = "lifetime" | "subscription";

interface PollComment {
  login: string;
  avatar: string;
  message: string;
  date: string;
}

interface PricingData {
  results: Record<Plan, Record<string, number>>;
  totals: Record<Plan, number>;
  comments: PollComment[];
  issueUrl: string;
  viewer: { login: string; avatar: string; votes: Record<Plan, string | null> } | null;
  loginEnabled: boolean;
}

const LOGIN_URL = "/api/auth/github?returnTo=/pricing/vote";
const LOGOUT_URL = "/api/auth/logout?returnTo=/pricing/vote";

const POLL_PLANS: { key: Plan; accent: string }[] = [
  { key: "lifetime", accent: "#38bdf8" },
  { key: "subscription", accent: "#22d3ee" },
];

// Option ids must match VALID_OPTIONS in api/pricing-vote.ts.
const POLL_OPTIONS: Record<Plan, { id: string; label: string }[]> = {
  lifetime: [
    { id: "lt-69", label: "¥69" },
    { id: "lt-99", label: "¥99" },
    { id: "lt-129", label: "¥129" },
    { id: "lt-199", label: "¥199" },
  ],
  subscription: [
    { id: "sub-3", label: "¥3" },
    { id: "sub-6", label: "¥6" },
    { id: "sub-10", label: "¥10" },
    { id: "sub-15plus", label: "¥15+" },
  ],
};

const POLL_TITLE_KEY: Record<Plan, keyof Messages> = {
  lifetime: "pricing.voteLifetime",
  subscription: "pricing.voteSubscription",
};

function GitHubIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.58.11.79-.25.79-.55 0-.27-.01-1.17-.02-2.12-3.2.7-3.87-1.36-3.87-1.36-.52-1.33-1.28-1.68-1.28-1.68-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.19 1.76 1.19 1.03 1.76 2.69 1.25 3.35.96.1-.75.4-1.25.72-1.54-2.55-.29-5.24-1.28-5.24-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.51-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11.04 11.04 0 0 1 5.78 0c2.2-1.49 3.17-1.18 3.17-1.18.62 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.69 5.38-5.26 5.67.41.35.77 1.05.77 2.12 0 1.53-.01 2.76-.01 3.14 0 .3.21.67.8.55A11.51 11.51 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5z" />
    </svg>
  );
}

export default function PricingVotePage() {
  const { t } = useLocale();
  const [data, setData] = useState<PricingData | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [authError, setAuthError] = useState(false);
  const [submittingPlan, setSubmittingPlan] = useState<Plan | null>(null);
  const [statusMsg, setStatusMsg] = useState<{ text: string; type: "success" | "error" } | null>(null);

  const [message, setMessage] = useState("");
  const [postingComment, setPostingComment] = useState(false);
  const [commentMsg, setCommentMsg] = useState<{ text: string; type: "success" | "error" } | null>(null);

  useEffect(() => {
    try {
      const params = new URLSearchParams(window.location.search);
      if (params.get("auth_error")) {
        setAuthError(true);
        window.history.replaceState(null, "", window.location.pathname);
      }
    } catch {
      // no window
    }

    fetch("/api/pricing-vote")
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then((d: PricingData) => setData(d))
      .catch(() => setLoadError(true))
      .finally(() => setLoading(false));
  }, []);

  const viewer = data?.viewer ?? null;

  const handleVote = useCallback(
    async (plan: Plan, option: string) => {
      if (!viewer) {
        window.location.href = LOGIN_URL;
        return;
      }
      if (viewer.votes[plan] || submittingPlan) return;

      setSubmittingPlan(plan);
      setStatusMsg(null);

      try {
        const res = await fetch("/api/pricing-vote", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ action: "vote", plan, option }),
        });

        if (res.status === 401) {
          window.location.href = LOGIN_URL;
          return;
        }
        if (res.status === 429) {
          setStatusMsg({ text: t("vote.rateLimited"), type: "error" });
          return;
        }
        if (!res.ok) {
          setStatusMsg({ text: t("vote.error"), type: "error" });
          return;
        }

        const result = await res.json();
        const finalOption = result.message === "already_voted" ? (result.option ?? option) : option;

        setData((prev) => {
          if (!prev || !prev.viewer) return prev;
          const next: PricingData = {
            ...prev,
            viewer: {
              ...prev.viewer,
              votes: { ...prev.viewer.votes, [plan]: finalOption },
            },
          };
          if (result.message === "voted") {
            next.results = {
              ...prev.results,
              [plan]: {
                ...prev.results[plan],
                [option]: (prev.results[plan][option] || 0) + 1,
              },
            };
            next.totals = { ...prev.totals, [plan]: prev.totals[plan] + 1 };
          }
          return next;
        });

        setStatusMsg({
          text: result.message === "already_voted" ? t("vote.alreadyVoted") : t("vote.success"),
          type: "success",
        });
      } catch {
        setStatusMsg({ text: t("vote.error"), type: "error" });
      } finally {
        setSubmittingPlan(null);
      }
    },
    [viewer, submittingPlan, t],
  );

  const handleComment = useCallback(async () => {
    const trimmed = message.trim();
    if (postingComment) return;
    if (!trimmed) {
      setCommentMsg({ text: t("pricing.messageRequired"), type: "error" });
      return;
    }

    setPostingComment(true);
    setCommentMsg(null);

    try {
      const res = await fetch("/api/pricing-vote", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "comment", message: trimmed }),
      });

      if (res.status === 401) {
        window.location.href = LOGIN_URL;
        return;
      }
      if (res.status === 429) {
        setCommentMsg({ text: t("vote.rateLimited"), type: "error" });
        return;
      }
      if (!res.ok) {
        setCommentMsg({ text: t("vote.error"), type: "error" });
        return;
      }

      const result = await res.json();
      setData((prev) =>
        prev ? { ...prev, comments: [result.comment, ...prev.comments] } : prev,
      );
      setMessage("");
      setCommentMsg({ text: t("pricing.commentSuccess"), type: "success" });
    } catch {
      setCommentMsg({ text: t("vote.error"), type: "error" });
    } finally {
      setPostingComment(false);
    }
  }, [message, postingComment, t]);

  const percentage = (plan: Plan, option: string): number => {
    if (!data || data.totals[plan] === 0) return 0;
    return Math.round(((data.results[plan][option] || 0) / data.totals[plan]) * 100);
  };

  const formatDate = (iso: string): string => {
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString();
  };

  return (
    <section className="pt-24 sm:pt-32 pb-16 sm:pb-20">
      <div className="mx-auto max-w-5xl px-4 sm:px-6 lg:px-8">
        {/* ── Header ── */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="text-center mb-10 sm:mb-12"
        >
          <a
            href="/pricing"
            className="inline-flex items-center gap-1.5 text-sm text-dark-text-muted hover:text-dark-text transition-colors mb-6"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M19 12H5" />
              <path d="m12 19-7-7 7-7" />
            </svg>
            {t("pricing.backToPricing")}
          </a>
          <h1 className="text-3xl sm:text-4xl font-bold text-dark-text tracking-tight">
            {t("pricing.voteTitle")}
          </h1>
          <p className="mt-4 text-sm sm:text-base text-dark-text-secondary max-w-2xl mx-auto leading-relaxed">
            {t("pricing.voteSubtitle")}
          </p>
        </motion.div>

        {authError && (
          <p className="mt-4 text-center text-sm font-medium text-danger">
            {t("pricing.authError")}
          </p>
        )}

        {loading && (
          <div className="flex items-center justify-center py-16">
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
          <div className="flex items-center justify-center py-16">
            <span className="text-sm text-danger">{t("vote.loadError")}</span>
          </div>
        )}

        {!loading && !loadError && data && (
          <>
            {/* Signed-in banner / sign-in prompt */}
            <div className="mt-6 flex items-center justify-center">
              {viewer ? (
                <span className="inline-flex items-center gap-2.5 rounded-full border border-dark-border bg-dark-surface1/50 pl-1.5 pr-4 py-1.5 text-sm text-dark-text-secondary">
                  {viewer.avatar ? (
                    <img src={viewer.avatar} alt="" className="w-6 h-6 rounded-full" />
                  ) : (
                    <GitHubIcon size={20} />
                  )}
                  <span>
                    {t("pricing.signedInAs")}{" "}
                    <span className="font-semibold text-dark-text">{viewer.login}</span>
                  </span>
                  <a href={LOGOUT_URL} className="text-xs text-dark-text-muted hover:text-dark-text underline underline-offset-2 transition-colors">
                    {t("pricing.logout")}
                  </a>
                </span>
              ) : (
                <a
                  href={LOGIN_URL}
                  className="inline-flex items-center gap-2 rounded-full border border-dark-border bg-dark-surface1/50 px-5 py-2 text-sm font-medium text-dark-text hover:border-dark-text-muted transition-colors"
                >
                  <GitHubIcon />
                  {t("pricing.signInToVote")}
                </a>
              )}
            </div>

            <div className="mt-6 grid grid-cols-1 sm:grid-cols-2 gap-4 sm:gap-6 max-w-3xl mx-auto">
              {POLL_PLANS.map((plan) => {
                const chosen = viewer?.votes[plan.key] ?? null;
                const hasVoted = chosen !== null;

                return (
                  <div key={plan.key} className="rounded-xl border border-dark-border bg-dark-surface1/30 p-5 sm:p-6">
                    <h3 className="text-sm font-semibold text-dark-text mb-4">
                      {t(POLL_TITLE_KEY[plan.key])}
                    </h3>
                    <div className="space-y-2.5">
                      {POLL_OPTIONS[plan.key].map((opt) => {
                        const count = data.results[plan.key][opt.id] || 0;
                        const pct = percentage(plan.key, opt.id);
                        const isChosen = chosen === opt.id;

                        return (
                          <button
                            key={opt.id}
                            type="button"
                            onClick={() => handleVote(plan.key, opt.id)}
                            disabled={hasVoted || submittingPlan !== null}
                            className={`relative w-full rounded-lg border px-4 py-2.5 text-left overflow-hidden transition-all duration-200 ${
                              isChosen
                                ? "border-brand-blue/50 ring-1 ring-brand-blue/30"
                                : hasVoted
                                  ? "border-dark-border/50 opacity-60 cursor-default"
                                  : "border-dark-border hover:border-dark-text-muted cursor-pointer"
                            }`}
                          >
                            <motion.div
                              className="absolute inset-y-0 left-0 opacity-15"
                              style={{ backgroundColor: plan.accent }}
                              initial={{ width: 0 }}
                              animate={{ width: `${pct}%` }}
                              transition={{ duration: 0.6, ease: [0.22, 1, 0.36, 1] }}
                            />
                            <span className="relative flex items-center justify-between">
                              <span className="flex items-center gap-2 text-sm font-medium text-dark-text tabular-nums">
                                {opt.label}
                                {isChosen && (
                                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke={plan.accent} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                                    <polyline points="20 6 9 17 4 12" />
                                  </svg>
                                )}
                              </span>
                              <span className="text-xs text-dark-text-muted tabular-nums">
                                {t("vote.votes", { n: String(count) })} · {pct}%
                              </span>
                            </span>
                          </button>
                        );
                      })}
                    </div>
                    <p className="mt-3 text-xs text-dark-text-muted tabular-nums">
                      {t("vote.totalVotes", { n: String(data.totals[plan.key]) })}
                    </p>
                  </div>
                );
              })}
            </div>

            {statusMsg && (
              <motion.p
                initial={{ opacity: 0, y: 5 }}
                animate={{ opacity: 1, y: 0 }}
                className={`mt-4 text-center text-sm font-medium ${statusMsg.type === "success" ? "text-success" : "text-danger"}`}
              >
                {statusMsg.text}
              </motion.p>
            )}

            {/* ── Discussion ── */}
            <div className="mt-20 sm:mt-24 max-w-3xl mx-auto">
              <div className="text-center">
                <h2 className="text-2xl sm:text-3xl font-bold text-dark-text tracking-tight">
                  {t("pricing.discussTitle")}
                </h2>
                <p className="mt-3 text-sm sm:text-base text-dark-text-secondary leading-relaxed">
                  {t("pricing.discussSubtitle")}
                </p>
              </div>

              {viewer ? (
                <div className="mt-8 rounded-xl border border-dark-border bg-dark-surface1/30 p-5 sm:p-6">
                  <div className="flex items-center gap-2.5 text-sm text-dark-text-secondary">
                    {viewer.avatar ? (
                      <img src={viewer.avatar} alt="" className="w-6 h-6 rounded-full" />
                    ) : (
                      <GitHubIcon size={20} />
                    )}
                    <span className="font-semibold text-dark-text">{viewer.login}</span>
                  </div>
                  <textarea
                    value={message}
                    onChange={(e) => setMessage(e.target.value)}
                    maxLength={500}
                    rows={3}
                    placeholder={t("pricing.messagePlaceholder")}
                    className="mt-3 w-full rounded-lg border border-dark-border bg-dark-surface2/50 px-3.5 py-2.5 text-sm text-dark-text placeholder:text-dark-text-muted focus:outline-none focus:border-brand-sky/50 transition-colors resize-y"
                  />
                  <div className="mt-3 flex items-center justify-between gap-3">
                    <span className="text-xs text-dark-text-muted tabular-nums">{message.length}/500</span>
                    <button
                      type="button"
                      onClick={handleComment}
                      disabled={postingComment}
                      className="inline-flex items-center gap-2 rounded-lg bg-brand-blue px-5 py-2 text-sm font-semibold text-white hover:opacity-90 disabled:opacity-50 transition-opacity"
                    >
                      {postingComment ? t("pricing.submittingComment") : t("pricing.submitComment")}
                    </button>
                  </div>
                  {commentMsg && (
                    <motion.p
                      initial={{ opacity: 0, y: 5 }}
                      animate={{ opacity: 1, y: 0 }}
                      className={`mt-3 text-sm font-medium ${commentMsg.type === "success" ? "text-success" : "text-danger"}`}
                    >
                      {commentMsg.text}
                    </motion.p>
                  )}
                </div>
              ) : (
                <div className="mt-8 rounded-xl border border-dashed border-dark-border bg-dark-surface1/20 p-8 text-center">
                  <p className="text-sm text-dark-text-muted mb-4">{t("pricing.signInToComment")}</p>
                  <a
                    href={LOGIN_URL}
                    className="inline-flex items-center gap-2 rounded-lg border border-dark-border bg-dark-surface1/50 px-5 py-2.5 text-sm font-medium text-dark-text hover:border-dark-text-muted transition-colors"
                  >
                    <GitHubIcon />
                    {t("pricing.signInWithGitHub")}
                  </a>
                </div>
              )}

              <div className="mt-6 space-y-3">
                {data.comments.length === 0 && (
                  <p className="text-center text-sm text-dark-text-muted py-8">
                    {t("pricing.noComments")}
                  </p>
                )}
                {data.comments.map((c, i) => (
                  <div
                    key={`${c.date}-${i}`}
                    className="rounded-lg border border-dark-border bg-dark-surface1/20 px-4 py-3"
                  >
                    <div className="flex items-center justify-between gap-3">
                      <a
                        href={`https://github.com/${c.login}`}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex items-center gap-2 text-sm font-medium text-dark-text hover:text-brand-sky transition-colors"
                      >
                        {c.avatar ? (
                          <img src={c.avatar} alt="" className="w-5 h-5 rounded-full" />
                        ) : (
                          <GitHubIcon size={16} />
                        )}
                        {c.login}
                      </a>
                      <span className="text-xs text-dark-text-muted tabular-nums">
                        {formatDate(c.date)}
                      </span>
                    </div>
                    <p className="mt-1.5 text-sm text-dark-text-secondary leading-relaxed whitespace-pre-wrap break-words">
                      {c.message}
                    </p>
                  </div>
                ))}
              </div>

              <p className="mt-8 text-center">
                <a
                  href={data.issueUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-2 text-sm text-dark-text-secondary hover:text-brand-sky transition-colors"
                >
                  <GitHubIcon />
                  {t("pricing.viewOnGitHub")}
                </a>
              </p>
            </div>
          </>
        )}
      </div>
    </section>
  );
}
