import { useLocale } from "@/lib/i18n";

export default function TermsSection() {
  const { t } = useLocale();

  return (
    <section className="relative py-20 sm:py-28 overflow-hidden bg-dark-bg">
      <div className="mx-auto max-w-3xl px-4 sm:px-6 lg:px-8 relative z-10">
        <div className="text-center mb-12">
          <h1 className="text-3xl sm:text-4xl font-bold tracking-tight text-dark-text">
            {t("terms.title")}
          </h1>
          <p className="mt-3 text-dark-text-muted text-sm">
            {t("terms.lastUpdated")}
          </p>
        </div>

        <div className="prose-custom space-y-8">
          {/* Introduction */}
          <div>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.intro")}
            </p>
          </div>

          {/* Section 1: Acceptance of Terms */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s1.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.s1.desc")}
            </p>
          </div>

          {/* Section 2: License */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s2.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm mb-3">
              {t("terms.s2.desc")}
            </p>
            <ul className="space-y-2 text-sm text-dark-text-secondary list-disc list-inside">
              <li>{t("terms.s2.item1")}</li>
              <li>{t("terms.s2.item2")}</li>
              <li>{t("terms.s2.item3")}</li>
            </ul>
          </div>

          {/* Section 3: Acceptable Use */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s3.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm mb-3">
              {t("terms.s3.desc")}
            </p>
            <ul className="space-y-2 text-sm text-dark-text-secondary list-disc list-inside">
              <li>{t("terms.s3.item1")}</li>
              <li>{t("terms.s3.item2")}</li>
              <li>{t("terms.s3.item3")}</li>
              <li>{t("terms.s3.item4")}</li>
            </ul>
          </div>

          {/* Section 4: Intellectual Property */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s4.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.s4.desc")}
            </p>
          </div>

          {/* Section 5: Disclaimer of Warranties */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s5.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.s5.desc")}
            </p>
          </div>

          {/* Section 6: Limitation of Liability */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s6.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.s6.desc")}
            </p>
          </div>

          {/* Section 7: User Content & Feedback */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s7.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.s7.desc")}
            </p>
          </div>

          {/* Section 8: Termination */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s8.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.s8.desc")}
            </p>
          </div>

          {/* Section 9: Changes to Terms */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s9.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.s9.desc")}
            </p>
          </div>

          {/* Section 10: Contact */}
          <div>
            <h2 className="text-lg font-semibold text-dark-text mb-3">
              {t("terms.s10.title")}
            </h2>
            <p className="text-dark-text-secondary leading-relaxed text-sm">
              {t("terms.s10.desc")}
            </p>
          </div>
        </div>
      </div>
    </section>
  );
}
