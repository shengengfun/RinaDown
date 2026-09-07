//! 关于：版本、软件更新、日志导出、浏览器扩展与捐赠链接。

use fluxdown_protocol::method;
use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::active_theme;
use gpui::{App, IntoElement as _, ParentElement, SharedString, Styled, div};
use gpui_component::{
    Icon, IconName, h_flex,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    v_flex,
};
use serde_json::json;

use super::SectionContext;

pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const WEBSITE: &str = "https://fluxdown.zerx.dev";
const CHROME_STORE: &str = "https://chromewebstore.google.com/search/FluxDown";
const FIREFOX_STORE: &str = "https://addons.mozilla.org/firefox/addon/fluxdown/";
const EDGE_STORE: &str = "https://microsoftedge.microsoft.com/addons/search/FluxDown";
const DONATE: &str = "https://fluxdown.zerx.dev/sponsor";

pub(crate) fn page(ctx: &SectionContext, _cx: &mut App) -> SettingPage {
    SettingPage::new(ctx.t("settingsCatAbout"))
        .icon(Icon::new(IconName::Info))
        .description(ctx.t("settingsCatAboutDesc"))
        .resettable(false)
        .group(version_group(ctx))
        .group(update_group(ctx))
        .group(logs_group(ctx))
        .group(links_group(ctx))
}

fn version_group(ctx: &SectionContext) -> SettingGroup {
    let version = SharedString::from(format!("v{APP_VERSION}"));
    SettingGroup::new()
        .title(SharedString::from("FluxDown"))
        .item(ctx.item(
            "currentVersion",
            None,
            SettingField::render(move |_, _, _| div().text_sm().child(version.clone())),
        ))
}

fn update_group(ctx: &SectionContext) -> SettingGroup {
    let channels = vec![
        (SharedString::from("stable"), ctx.t("updateChannelStable")),
        (
            SharedString::from("frontier"),
            ctx.t("updateChannelFrontier"),
        ),
    ];
    SettingGroup::new()
        .title(ctx.t("softwareUpdate"))
        .item(ctx.item(
            "updateChannel",
            Some("updateChannelDesc"),
            ctx.pref_dropdown("general.update_channel", "stable", channels),
        ))
        .item(ctx.item(
            "autoCheckUpdate",
            Some("autoCheckUpdateDesc"),
            ctx.pref_switch("general.auto_check_update", true),
        ))
        .item(ctx.item(
            "checkUpdate",
            Some("checkUpdateDesc"),
            check_update_field(ctx),
        ))
        .item(release_notes_item(ctx))
}

fn check_update_field(ctx: &SectionContext) -> SettingField<SharedString> {
    let store = ctx.store();
    let translator = ctx.translator.clone();
    let check = ctx.t("checkUpdate");
    let latest = ctx.t("latestVersion");
    SettingField::render(move |options, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let busy = store.read(cx).is_busy("update");
        let result = store.read(cx).update_check().cloned();
        let status = result.as_ref().map(|result| {
            if result.has_update {
                translator.text_with(
                    "updatePromptBody",
                    &[("v", &result.latest_version), ("size", "")],
                )
            } else {
                format!("{latest}: v{}", result.latest_version)
            }
        });
        let click_store = store.clone();
        let download_url = result
            .as_ref()
            .filter(|result| result.has_update)
            .map(|result| {
                if result.download_url.is_empty() {
                    result.release_page_url.clone()
                } else {
                    result.download_url.clone()
                }
            })
            .filter(|url| !url.is_empty());
        let update_now = translator.text("updateNow").to_owned();
        h_flex()
            .gap(tokens.spacing.sm)
            .items_center()
            .children(status.map(|status| {
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(SharedString::from(status))
            }))
            .children(download_url.map(|url| {
                button(
                    "about-update-now",
                    SharedString::from(update_now.clone()),
                    ButtonVariant::Primary,
                    cx,
                )
                .on_click(move |_, _, cx| cx.open_url(&url))
            }))
            .child(
                button(
                    "about-check-update",
                    check.clone(),
                    ButtonVariant::Secondary,
                    cx,
                )
                .disabled(options.is_disabled() || busy)
                .on_click(move |_, _, cx| {
                    click_store.update(cx, |store, cx| {
                        let channel = store.pref_str("general.update_channel", "stable");
                        store.check_update(Some(channel), cx);
                    });
                }),
            )
    })
}

fn release_notes_item(ctx: &SectionContext) -> SettingItem {
    let store = ctx.store();
    SettingItem::render(move |_, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let Some(result) = store.read(cx).update_check().cloned() else {
            return div().into_any_element();
        };
        if result.notes.is_empty() {
            return div().into_any_element();
        }
        v_flex()
            .w_full()
            .gap(tokens.spacing.sm)
            .children(result.notes.into_iter().take(10).map(|note| {
                v_flex()
                    .gap(tokens.spacing.xxs)
                    .child(div().text_sm().child(SharedString::from(format!(
                        "v{} {}",
                        note.version, note.published_at
                    ))))
                    .child(
                        div()
                            .text_xs()
                            .text_color(tokens.colors.muted_foreground)
                            .child(SharedString::from(note.body)),
                    )
            }))
            .into_any_element()
    })
}

fn logs_group(ctx: &SectionContext) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("logExport"))
        .description(ctx.t("logExportDesc"))
        .item(ctx.item(
            "logMaxSize",
            Some("logMaxSizeDesc"),
            ctx.pref_number("log_max_size_mb", 10, 1, 1024),
        ))
        .item(ctx.item("logExportButton", None, export_field(ctx)))
}

fn export_field(ctx: &SectionContext) -> SettingField<SharedString> {
    let store = ctx.store();
    let export = ctx.t("logExportButton");
    let open = ctx.t("doctorActionOpenLogDir");
    SettingField::render(move |options, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let busy = store.read(cx).is_busy("logExport");
        let export_store = store.clone();
        let open_store = store.clone();
        h_flex()
            .gap(tokens.spacing.sm)
            .child(
                button(
                    "about-open-log-dir",
                    open.clone(),
                    ButtonVariant::Secondary,
                    cx,
                )
                .disabled(options.is_disabled())
                .on_click(move |_, _, cx| {
                    open_store.update(cx, |store, cx| {
                        store.call_with(
                            "logExport",
                            method::AGENT_DIAGNOSTICS_LOG_PATHS,
                            json!({}),
                            cx,
                            |store, result, cx| {
                                if let Ok(value) = result
                                    && let Some(dir) = value
                                        .get("agentLogDir")
                                        .and_then(serde_json::Value::as_str)
                                        .filter(|dir| !dir.is_empty())
                                {
                                    store.call_simple(
                                        "logExport",
                                        method::AGENT_PLATFORM_OPEN_PATH,
                                        json!({ "path": dir, "reveal": false }),
                                        None,
                                        cx,
                                    );
                                }
                            },
                        );
                    });
                }),
            )
            .child(
                button(
                    "about-export-logs",
                    export.clone(),
                    ButtonVariant::Primary,
                    cx,
                )
                .disabled(options.is_disabled() || busy)
                .on_click(move |_, _, cx| {
                    let store = export_store.clone();
                    let receiver =
                        cx.prompt_for_new_path(&std::env::temp_dir(), Some("fluxdown-logs.zip"));
                    cx.spawn(async move |cx| {
                        if let Ok(Ok(Some(path))) = receiver.await {
                            let target = path.display().to_string();
                            store.update(cx, |store, cx| {
                                store.call_simple(
                                    "logExport",
                                    method::AGENT_DIAGNOSTICS_EXPORT_LOGS,
                                    json!({ "targetPath": target }),
                                    Some("logExportSuccessNotice"),
                                    cx,
                                );
                            });
                        }
                    })
                    .detach();
                }),
            )
    })
}

fn links_group(ctx: &SectionContext) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("extensionCardTitle"))
        .description(ctx.t("extensionCardDesc"))
        .item(link_item(
            ctx,
            "extensionCardTitle",
            &[
                ("Chrome", CHROME_STORE),
                ("Firefox", FIREFOX_STORE),
                ("Edge", EDGE_STORE),
            ],
        ))
        .item(link_item(
            ctx,
            "donateTitle",
            &[("donateButton", DONATE), ("website", WEBSITE)],
        ))
}

fn link_item(
    ctx: &SectionContext,
    title_key: &str,
    links: &[(&'static str, &'static str)],
) -> SettingItem {
    let links: Vec<(SharedString, &'static str)> = links
        .iter()
        .map(|(label, url)| {
            let text = ctx.translator.text(label);
            (SharedString::from(text.to_owned()), *url)
        })
        .collect();
    let title = ctx.t(title_key);
    SettingItem::render(move |_, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap(tokens.spacing.md)
            .child(div().text_sm().child(title.clone()))
            .child(
                h_flex()
                    .gap(tokens.spacing.sm)
                    .children(links.iter().map(|(label, url)| {
                        let url = *url;
                        button(
                            SharedString::from(format!("about-link-{url}")),
                            label.clone(),
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .on_click(move |_, _, cx| cx.open_url(url))
                    })),
            )
            .into_any_element()
    })
    .keywords([title_key.to_owned()])
}
