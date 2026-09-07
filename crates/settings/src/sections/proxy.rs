//! 代理：模式、手动服务器、连通性测试、站点凭据。

use fluxdown_protocol::method;
use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::active_theme;
use gpui::{App, ParentElement, SharedString, Styled, div};
use gpui_component::{
    Icon, IconName, h_flex,
    setting::{SettingField, SettingGroup, SettingPage},
};
use serde_json::json;

use super::{SectionContext, site_auth};

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingPage {
    let mode = ctx.store.read(cx).daemon_str("proxy_mode");
    let mut page = SettingPage::new(ctx.t("settingsCatProxy"))
        .icon(Icon::new(IconName::Globe))
        .description(ctx.t("settingsCatProxyDesc"))
        .group(mode_group(ctx));
    if matches!(mode.as_str(), "manual" | "auto") {
        page = page.group(manual_group(ctx));
    }
    page.group(site_auth::group(ctx, cx))
}

fn mode_group(ctx: &SectionContext) -> SettingGroup {
    let options = vec![
        (SharedString::from("none"), ctx.t("proxyModeNone")),
        (SharedString::from("system"), ctx.t("proxyModeSystem")),
        (SharedString::from("manual"), ctx.t("proxyModeManual")),
        (SharedString::from("auto"), ctx.t("proxyModeAuto")),
    ];
    SettingGroup::new()
        .title(ctx.t("proxySettings"))
        .description(ctx.t("proxyBtNote"))
        .item(ctx.item(
            "proxySettings",
            Some("proxySettingsDesc"),
            ctx.daemon_dropdown("proxy_mode", options),
        ))
}

fn manual_group(ctx: &SectionContext) -> SettingGroup {
    let types = vec![
        (SharedString::from("http"), SharedString::from("HTTP")),
        (SharedString::from("https"), SharedString::from("HTTPS")),
        (SharedString::from("socks4"), SharedString::from("SOCKS4")),
        (SharedString::from("socks5"), SharedString::from("SOCKS5")),
    ];
    SettingGroup::new()
        .title(ctx.t("proxyModeManual"))
        .description(ctx.t("proxyModeManualDesc"))
        .item(ctx.item("proxyType", None, ctx.daemon_dropdown("proxy_type", types)))
        .item(ctx.item(
            "proxyHost",
            Some("proxyHostPlaceholder"),
            ctx.daemon_input("proxy_host"),
        ))
        .item(ctx.item(
            "proxyPort",
            Some("proxyPortPlaceholder"),
            ctx.daemon_input("proxy_port"),
        ))
        .item(ctx.item(
            "proxyUsername",
            Some("proxyUsernamePlaceholder"),
            ctx.daemon_input("proxy_username"),
        ))
        .item(ctx.item(
            "proxyPassword",
            Some("proxyPasswordPlaceholder"),
            ctx.daemon_input("proxy_password"),
        ))
        .item(ctx.item(
            "proxyNoList",
            Some("proxyNoListDesc"),
            ctx.daemon_input("proxy_no_list"),
        ))
        .item(ctx.item("proxyTestConnection", None, test_field(ctx)))
}

fn test_field(ctx: &SectionContext) -> SettingField<SharedString> {
    let store = ctx.store();
    let label = ctx.t("proxyTestConnection");
    let testing = ctx.t("proxyTesting");
    let translator = ctx.translator.clone();
    SettingField::render(move |_, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let busy = store.read(cx).is_busy("proxyTest");
        let result = store
            .read(cx)
            .transient("proxy_test_result")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let click_store = store.clone();
        h_flex()
            .gap(tokens.spacing.sm)
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(SharedString::from(result.unwrap_or_default())),
            )
            .child(
                button(
                    "proxy-test",
                    if busy { testing.clone() } else { label.clone() },
                    ButtonVariant::Secondary,
                    cx,
                )
                .disabled(busy)
                .on_click({
                    let translator = translator.clone();
                    move |_, _, cx| {
                        let translator = translator.clone();
                        click_store.update(cx, |store, cx| {
                            let params = json!({
                                "proxyType": store.daemon_str("proxy_type"),
                                "host": store.daemon_str("proxy_host"),
                                "port": store.daemon_str("proxy_port"),
                                "username": store.daemon_str("proxy_username"),
                                "password": store.daemon_str("proxy_password"),
                            });
                            store.call_with(
                                "proxyTest",
                                method::DAEMON_CONFIG_PROXY_TEST,
                                params,
                                cx,
                                move |store, result, cx| {
                                    let text = match result {
                                        Ok(value) => {
                                            let ms = value
                                                .get("latencyMs")
                                                .and_then(serde_json::Value::as_i64)
                                                .unwrap_or(0)
                                                .to_string();
                                            translator.text_with("proxyTestSuccess", &[("ms", &ms)])
                                        }
                                        Err(error) => translator.text_with(
                                            "proxyTestFailed",
                                            &[("error", &format!("{:?}", error.code))],
                                        ),
                                    };
                                    store.set_transient("proxy_test_result", json!(text), cx);
                                },
                            );
                        });
                    }
                }),
            )
    })
}
