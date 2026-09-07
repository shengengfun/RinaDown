/**
 * 文档页反馈表单(client:visible):文字建议 → POST /api/feedback(type: "docs")。
 * 文案按页面语言(URL 段)内联双语,不依赖全站 useLocale(docs 页语言由 URL 决定)。
 */
import { useState } from "react";
import type { FormEvent } from "react";

interface Props {
  pagePath: string;
  lang: "en" | "zh";
}

type Status = "idle" | "sending" | "done" | "error";

export default function DocsFeedbackForm({ pagePath, lang }: Props) {
  const [text, setText] = useState("");
  const [contact, setContact] = useState("");
  const [status, setStatus] = useState<Status>("idle");
  const zh = lang === "zh";

  const t = {
    heading: zh ? "这页文档有问题?" : "Something wrong with this page?",
    placeholder: zh
      ? "告诉我们哪里可以改进(错误、缺失、过时内容……)"
      : "Tell us what could be improved (mistakes, gaps, outdated content…)",
    contact: zh ? "联系方式(可选,便于回复)" : "Contact (optional, for follow-up)",
    submit: zh ? "提交建议" : "Send feedback",
    sending: zh ? "提交中…" : "Sending…",
    done: zh ? "已收到,感谢反馈!我们会在 GitHub Issue 中跟进。" : "Received — thanks! We'll follow up in a GitHub issue.",
    error: zh ? "提交失败,请稍后再试。" : "Failed to send, please try again later.",
  };

  async function submit(e: FormEvent) {
    e.preventDefault();
    const description = text.trim();
    if (!description || status === "sending") return;
    setStatus("sending");
    try {
      const res = await fetch("/api/feedback", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          type: "docs",
          title: `[Docs] ${pagePath}`,
          description,
          contact: contact.trim() || undefined,
          pagePath,
        }),
      });
      if (!res.ok) throw new Error(String(res.status));
      setStatus("done");
      setText("");
      setContact("");
    } catch {
      setStatus("error");
    }
  }

  if (status === "done") {
    return <p className="mt-8 text-sm text-success">{t.done}</p>;
  }

  return (
    <form onSubmit={submit} className="mt-8 rounded-lg border border-dark-border bg-dark-surface1 p-4">
      <h3 className="text-sm font-medium text-dark-text">{t.heading}</h3>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={t.placeholder}
        rows={3}
        maxLength={2000}
        required
        className="mt-3 w-full resize-y rounded-md border border-dark-border bg-dark-bg px-3 py-2 text-sm text-dark-text placeholder:text-dark-text-muted focus:border-brand-sky/60 focus:outline-none"
      />
      <div className="mt-2 flex flex-col gap-2 sm:flex-row">
        <input
          type="text"
          value={contact}
          onChange={(e) => setContact(e.target.value)}
          placeholder={t.contact}
          maxLength={200}
          className="flex-1 rounded-md border border-dark-border bg-dark-bg px-3 py-2 text-sm text-dark-text placeholder:text-dark-text-muted focus:border-brand-sky/60 focus:outline-none"
        />
        <button
          type="submit"
          disabled={status === "sending" || text.trim().length === 0}
          className="rounded-md bg-brand-sky/10 px-4 py-2 text-sm font-medium text-brand-sky transition-colors hover:bg-brand-sky/20 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {status === "sending" ? t.sending : t.submit}
        </button>
      </div>
      {status === "error" && <p className="mt-2 text-xs text-danger">{t.error}</p>}
    </form>
  );
}
