//! Doctor：环境自检报告与就地修复。检查项 `id`/`hint`/`repair.action` 由 agent 给出。

use fluxdown_protocol::{DiagnosticLevel, DiagnosticRepairParams, method};
use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::active_theme;
use gpui::{App, ClipboardItem, IntoElement as _, ParentElement, SharedString, Styled, div};
use gpui_component::{
    Icon, IconName, h_flex,
    setting::{SettingGroup, SettingItem, SettingPage},
    v_flex,
};
use serde_json::json;

use super::{SectionContext, camel};

pub(crate) fn page(ctx: &SectionContext, _cx: &mut App) -> SettingPage {
    SettingPage::new(ctx.t("settingsCatDoctor"))
        .icon(Icon::new(IconName::Info))
        .description(ctx.t("settingsCatDoctorDesc"))
        .resettable(false)
        .group(
            SettingGroup::new()
                .title(ctx.t("doctorTitle"))
                .description(ctx.t("doctorDesc"))
                .item(toolbar_item(ctx))
                .item(report_item(ctx)),
        )
}

fn toolbar_item(ctx: &SectionContext) -> SettingItem {
    let store = ctx.store();
    let translator = ctx.translator.clone();
    let run = ctx.t("doctorRun");
    let running = ctx.t("doctorRunning");
    let copy = ctx.t("doctorCopyReport");
    let never = ctx.t("doctorNeverRun");
    SettingItem::render(move |options, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let busy = store.read(cx).is_busy("diagnostics");
        let report = store.read(cx).diagnostics().cloned();
        let summary = report.as_ref().map_or_else(
            || never.to_string(),
            |report| {
                let issues = report
                    .checks
                    .iter()
                    .filter(|check| {
                        matches!(check.level, DiagnosticLevel::Warn | DiagnosticLevel::Error)
                    })
                    .count();
                if issues == 0 {
                    translator.text("doctorAllHealthy").to_owned()
                } else {
                    translator.text_with("doctorIssuesFound", &[("n", &issues.to_string())])
                }
            },
        );
        let run_store = store.clone();
        let copy_report = report.clone();
        let copy_translator = translator.clone();
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap(tokens.spacing.md)
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.colors.muted_foreground)
                    .child(SharedString::from(summary)),
            )
            .child(
                h_flex()
                    .gap(tokens.spacing.sm)
                    .child(
                        button("doctor-copy", copy.clone(), ButtonVariant::Secondary, cx)
                            .disabled(options.is_disabled() || copy_report.is_none())
                            .on_click(move |_, _, cx| {
                                if let Some(report) = &copy_report {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        render_report(report, &copy_translator),
                                    ));
                                }
                            }),
                    )
                    .child(
                        button(
                            "doctor-run",
                            if busy { running.clone() } else { run.clone() },
                            ButtonVariant::Primary,
                            cx,
                        )
                        .disabled(options.is_disabled() || busy)
                        .on_click(move |_, _, cx| {
                            run_store.update(cx, |store, cx| store.run_diagnostics(cx));
                        }),
                    ),
            )
            .into_any_element()
    })
    .keywords([ctx.t("doctorRun"), ctx.t("doctorCopyReport")])
}

fn report_item(ctx: &SectionContext) -> SettingItem {
    let store = ctx.store();
    let translator = ctx.translator.clone();
    SettingItem::render(move |options, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let Some(report) = store.read(cx).diagnostics().cloned() else {
            return div().into_any_element();
        };
        let busy = store.read(cx).is_busy("diagnostics");
        let mut column = v_flex().w_full().gap(tokens.spacing.xs);
        for check in &report.checks {
            let label_key = format!("doctorCheck{}", camel(&check.id));
            let mut title = translator.text(&label_key).to_owned();
            if !check.target.is_empty() {
                title.push_str(" · ");
                title.push_str(&check.target);
            }
            let level_key = match check.level {
                DiagnosticLevel::Ok => "doctorLevelOk",
                DiagnosticLevel::Warn => "doctorLevelWarn",
                DiagnosticLevel::Error => "doctorLevelError",
                DiagnosticLevel::Info => "doctorLevelInfo",
            };
            let level_color = match check.level {
                DiagnosticLevel::Ok => tokens.colors.primary,
                DiagnosticLevel::Warn => tokens.colors.accent_foreground,
                DiagnosticLevel::Error => tokens.colors.destructive,
                DiagnosticLevel::Info => tokens.colors.muted_foreground,
            };
            let hint = (!check.hint.is_empty()).then(|| {
                translator
                    .text(&format!("doctorHint{}", camel(&check.hint)))
                    .to_owned()
            });
            let mut row = h_flex()
                .w_full()
                .items_start()
                .gap(tokens.spacing.sm)
                .px(tokens.spacing.sm)
                .py(tokens.spacing.xs)
                .rounded(tokens.radius.md)
                .border_1()
                .border_color(tokens.colors.border)
                .child(
                    div()
                        .min_w_16()
                        .text_xs()
                        .text_color(level_color)
                        .child(SharedString::from(translator.text(level_key).to_owned())),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(tokens.spacing.xxs)
                        .child(div().text_sm().child(SharedString::from(title)))
                        .child(
                            div()
                                .text_xs()
                                .text_color(tokens.colors.muted_foreground)
                                .child(SharedString::from(check.detail.clone())),
                        )
                        .children(hint.map(|hint| {
                            div()
                                .text_xs()
                                .text_color(tokens.colors.accent_foreground)
                                .child(SharedString::from(hint))
                        })),
                );
            if let Some(repair) = &check.repair {
                let action_key = format!("doctorAction{}", camel(&repair.action));
                let label = SharedString::from(translator.text(&action_key).to_owned());
                let repair_store = store.clone();
                let params = repair.clone();
                row = row.child(
                    button(
                        SharedString::from(format!("doctor-repair-{}-{}", check.id, check.target)),
                        label,
                        ButtonVariant::Secondary,
                        cx,
                    )
                    .disabled(options.is_disabled() || busy)
                    .on_click(move |_, _, cx| {
                        let params = params.clone();
                        repair_store.update(cx, |store, cx| run_repair(store, params, cx));
                    }),
                );
            }
            column = column.child(row);
        }
        column.into_any_element()
    })
}

fn run_repair(
    store: &mut crate::store::SettingsStore,
    params: DiagnosticRepairParams,
    cx: &mut gpui::Context<crate::store::SettingsStore>,
) {
    if params.action == "openLogDir" {
        store.call_simple(
            "diagnostics",
            method::AGENT_PLATFORM_OPEN_PATH,
            json!({ "path": params.target, "reveal": false }),
            None,
            cx,
        );
        return;
    }
    store.repair_diagnostics(params, cx);
}

/// 纯文本报告（供复制到反馈）。
fn render_report(
    report: &fluxdown_protocol::DiagnosticsReportDto,
    translator: &fluxdown_ui_i18n::Translator,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "FluxDown {} · {} · {}\nagent data dir: {}\ndaemon connected: {}\n\n",
        report.app_version,
        report.platform,
        report.generated_at_unix_ms,
        report.agent_data_dir,
        report.daemon_connected
    ));
    for check in &report.checks {
        let level = match check.level {
            DiagnosticLevel::Ok => "OK",
            DiagnosticLevel::Warn => "WARN",
            DiagnosticLevel::Error => "ERROR",
            DiagnosticLevel::Info => "INFO",
        };
        let label_key = format!("doctorCheck{}", camel(&check.id));
        let label = translator.text(&label_key);
        out.push_str(&format!("[{level}] {label}"));
        if !check.target.is_empty() {
            out.push_str(&format!(" ({})", check.target));
        }
        out.push_str(&format!(": {}\n", check.detail));
        if !check.hint.is_empty() {
            out.push_str(&format!("  hint: {}\n", check.hint));
        }
    }
    out
}
