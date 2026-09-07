//! Webhook：端点列表（daemon `webhook.endpoints` JSON）与投递记录。

use fluxdown_protocol::method;
use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::active_theme;
use gpui::{App, Context, IntoElement as _, ParentElement, SharedString, Styled, div};
use gpui_component::{
    Disableable as _, h_flex,
    setting::{SettingGroup, SettingItem},
    v_flex,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

use super::{SectionContext, webhook_dialog};
use crate::store::SettingsStore;

pub(crate) const ENDPOINTS_KEY: &str = "webhook.endpoints";

/// 与 `engine::webhook::EndpointSpec` 同 wire 形状。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct EndpointSpec {
    pub id: String,
    pub name: String,
    pub preset: String,
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub events: Vec<String>,
    pub queue_id: String,
    pub headers: BTreeMap<String, String>,
    pub body_template: String,
    pub sign_secret: String,
    pub allow_http: bool,
    pub use_proxy: bool,
}

fn default_true() -> bool {
    true
}

/// 事件 wire 名（与引擎 `WebhookEventKind::wire()` 逐字一致）。
pub(crate) const WEBHOOK_EVENTS: &[(&str, &str)] = &[
    ("task.created", "webhookEventCreated"),
    ("task.started", "webhookEventStarted"),
    ("task.completed", "webhookEventCompleted"),
    ("task.failed", "webhookEventFailed"),
    ("task.paused", "webhookEventPaused"),
    ("queue.drained", "webhookEventQueueDrained"),
];

pub(crate) fn read_endpoints(store: &SettingsStore) -> Vec<EndpointSpec> {
    let raw = store.daemon_str(ENDPOINTS_KEY);
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str(&raw).unwrap_or_default()
}

pub(crate) fn write_endpoints(
    store: &mut SettingsStore,
    list: &[EndpointSpec],
    cx: &mut Context<SettingsStore>,
) {
    let encoded = serde_json::to_string(list).unwrap_or_else(|_| "[]".to_owned());
    store.set_daemon(ENDPOINTS_KEY, encoded, cx);
}

pub(crate) fn endpoints_group(ctx: &SectionContext, _cx: &mut App) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("notifyGroupWebhook"))
        .description(ctx.t("webhookSemantics"))
        .item(endpoints_item(ctx))
}

fn endpoints_item(ctx: &SectionContext) -> SettingItem {
    let store = ctx.store();
    let translator = ctx.translator.clone();
    let empty_title = ctx.t("webhookEmptyTitle");
    let empty_desc = ctx.t("webhookEmptyDesc");
    let add = ctx.t("webhookAddEndpoint");
    let edit = ctx.t("webhookRowEdit");
    let test = ctx.t("webhookRowTest");
    let delete = ctx.t("webhookRowDelete");
    let disabled_label = ctx.t("webhookHealthDisabled");
    SettingItem::render(move |options, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let disabled = options.is_disabled();
        let endpoints = read_endpoints(store.read(cx));
        let deliveries = store.read(cx).webhook_deliveries().to_vec();
        let mut column = v_flex().w_full().gap(tokens.spacing.xs);
        if endpoints.is_empty() {
            column = column
                .child(div().text_sm().child(empty_title.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(tokens.colors.muted_foreground)
                        .child(empty_desc.clone()),
                );
        }
        for endpoint in &endpoints {
            let health = deliveries
                .iter()
                .filter(|delivery| delivery.endpoint_id == endpoint.id)
                .max_by_key(|delivery| delivery.timestamp_ms)
                .map_or_else(
                    || translator.text("webhookHealthNone").to_owned(),
                    |delivery| {
                        if delivery.success {
                            translator.text_with(
                                "webhookHealthOk",
                                &[("time", &format!("{}ms", delivery.latency_ms))],
                            )
                        } else {
                            translator
                                .text_with("webhookHealthFail", &[("detail", &delivery.error)])
                        }
                    },
                );
            let toggle_store = store.clone();
            let toggle_id = endpoint.id.clone();
            let edit_store = store.clone();
            let edit_translator = translator.clone();
            let edit_endpoint = endpoint.clone();
            let test_store = store.clone();
            let test_endpoint = endpoint.clone();
            let delete_store = store.clone();
            let delete_id = endpoint.id.clone();
            let events = endpoint.events.join(", ");
            column = column.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(tokens.spacing.sm)
                    .px(tokens.spacing.sm)
                    .py(tokens.spacing.xs)
                    .rounded(tokens.radius.md)
                    .border_1()
                    .border_color(tokens.colors.border)
                    .child(
                        gpui_component::switch::Switch::new(SharedString::from(format!(
                            "webhook-enabled-{}",
                            endpoint.id
                        )))
                        .checked(endpoint.enabled)
                        .disabled(disabled)
                        .on_click(move |checked: &bool, _, cx| {
                            let id = toggle_id.clone();
                            let checked = *checked;
                            toggle_store.update(cx, |store, cx| {
                                let mut list = read_endpoints(store);
                                if let Some(entry) = list.iter_mut().find(|entry| entry.id == id) {
                                    entry.enabled = checked;
                                    write_endpoints(store, &list, cx);
                                }
                            });
                        }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(tokens.spacing.xxs)
                            .child(
                                div()
                                    .text_sm()
                                    .child(SharedString::from(endpoint.name.clone())),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(SharedString::from(format!(
                                        "{} · {}",
                                        endpoint.url, events
                                    ))),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(if endpoint.enabled {
                                        SharedString::from(health)
                                    } else {
                                        disabled_label.clone()
                                    }),
                            ),
                    )
                    .child(
                        button(
                            SharedString::from(format!("webhook-edit-{}", endpoint.id)),
                            edit.clone(),
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .disabled(disabled)
                        .on_click(move |_, window, cx| {
                            webhook_dialog::open(
                                edit_store.clone(),
                                edit_translator.clone(),
                                Some(edit_endpoint.clone()),
                                window,
                                cx,
                            );
                        }),
                    )
                    .child(
                        button(
                            SharedString::from(format!("webhook-test-{}", endpoint.id)),
                            test.clone(),
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .disabled(disabled || store.read(cx).is_busy("webhookTest"))
                        .on_click({
                            let translator = translator.clone();
                            move |_, _, cx| {
                                let params = serde_json::to_value(&test_endpoint)
                                    .unwrap_or_else(|_| json!({}));
                                let translator = translator.clone();
                                test_store.update(cx, |store, cx| {
                                    store.call_with(
                                        "webhookTest",
                                        method::DAEMON_WEBHOOK_TEST,
                                        params,
                                        cx,
                                        move |store, result, cx| {
                                            let text = match result {
                                                Ok(value) => {
                                                    let success = value
                                                        .get("success")
                                                        .and_then(serde_json::Value::as_bool)
                                                        .unwrap_or(false);
                                                    let status = value
                                                        .get("statusCode")
                                                        .and_then(serde_json::Value::as_i64)
                                                        .unwrap_or(0)
                                                        .to_string();
                                                    let ms = value
                                                        .get("latencyMs")
                                                        .and_then(serde_json::Value::as_i64)
                                                        .unwrap_or(0)
                                                        .to_string();
                                                    let error = value
                                                        .get("error")
                                                        .and_then(serde_json::Value::as_str)
                                                        .unwrap_or_default()
                                                        .to_owned();
                                                    if success {
                                                        translator.text_with(
                                                            "webhookTestOk",
                                                            &[("status", &status), ("ms", &ms)],
                                                        )
                                                    } else {
                                                        translator.text_with(
                                                            "webhookTestFail",
                                                            &[("error", &error)],
                                                        )
                                                    }
                                                }
                                                Err(error) => translator.text_with(
                                                    "webhookTestFail",
                                                    &[("error", &format!("{:?}", error.code))],
                                                ),
                                            };
                                            store.set_transient(
                                                "webhook_test_result",
                                                json!(text),
                                                cx,
                                            );
                                        },
                                    );
                                });
                            }
                        }),
                    )
                    .child(
                        button(
                            SharedString::from(format!("webhook-delete-{}", endpoint.id)),
                            delete.clone(),
                            ButtonVariant::Destructive,
                            cx,
                        )
                        .disabled(disabled)
                        .on_click(move |_, _, cx| {
                            let id = delete_id.clone();
                            delete_store.update(cx, |store, cx| {
                                let list: Vec<EndpointSpec> = read_endpoints(store)
                                    .into_iter()
                                    .filter(|entry| entry.id != id)
                                    .collect();
                                write_endpoints(store, &list, cx);
                            });
                        }),
                    ),
            );
        }
        if let Some(text) = store
            .read(cx)
            .transient("webhook_test_result")
            .and_then(serde_json::Value::as_str)
        {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(SharedString::from(text.to_owned())),
            );
        }
        let add_store = store.clone();
        let add_translator = translator.clone();
        column
            .child(
                h_flex().w_full().justify_end().child(
                    button("webhook-add", add.clone(), ButtonVariant::Primary, cx)
                        .disabled(disabled)
                        .on_click(move |_, window, cx| {
                            webhook_dialog::open(
                                add_store.clone(),
                                add_translator.clone(),
                                None,
                                window,
                                cx,
                            );
                        }),
                ),
            )
            .into_any_element()
    })
    .keywords([ctx.t("notifyGroupWebhook"), ctx.t("webhookAddEndpoint")])
}

pub(crate) fn delivery_log_group(ctx: &SectionContext, _cx: &mut App) -> SettingGroup {
    let store = ctx.store();
    let translator = ctx.translator.clone();
    let empty = ctx.t("webhookLogEmpty");
    let clear = ctx.t("webhookLogClear");
    let simulate = ctx.t("webhookLogSimulate");
    let item = SettingItem::render(move |options, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let disabled = options.is_disabled();
        let deliveries = store.read(cx).webhook_deliveries().to_vec();
        let clear_store = store.clone();
        let simulate_store = store.clone();
        let mut column = v_flex().w_full().gap(tokens.spacing.xs);
        if deliveries.is_empty() {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(tokens.colors.muted_foreground)
                    .child(empty.clone()),
            );
        }
        for delivery in deliveries.iter().rev().take(50) {
            let status = if delivery.success {
                format!("{} · {}ms", delivery.status_code, delivery.latency_ms)
            } else if delivery.error.is_empty() {
                format!("HTTP {}", delivery.status_code)
            } else {
                delivery.error.clone()
            };
            let attempts =
                translator.text_with("webhookAttempts", &[("n", &delivery.attempts.to_string())]);
            column = column.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap(tokens.spacing.md)
                    .py(tokens.spacing.xxs)
                    .border_b_1()
                    .border_color(tokens.colors.border)
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap(tokens.spacing.xxs)
                            .child(div().text_sm().child(SharedString::from(format!(
                                "{} · {}",
                                delivery.endpoint_name, delivery.event
                            ))))
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(SharedString::from(format!(
                                        "{} · {}",
                                        status, attempts
                                    ))),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if delivery.success {
                                tokens.colors.muted_foreground
                            } else {
                                tokens.colors.destructive
                            })
                            .child(SharedString::from(super::subscription::format_unix(
                                delivery.timestamp_ms / 1000,
                            ))),
                    ),
            );
        }
        column
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(tokens.spacing.sm)
                    .child(
                        button(
                            "webhook-simulate",
                            simulate.clone(),
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .disabled(disabled || simulate_store.read(cx).is_busy("webhookSimulate"))
                        .on_click(move |_, _, cx| {
                            simulate_store.update(cx, |store, cx| {
                                store.call_simple(
                                    "webhookSimulate",
                                    method::DAEMON_WEBHOOK_SIMULATE,
                                    json!({}),
                                    None,
                                    cx,
                                );
                            });
                        }),
                    )
                    .child(
                        button(
                            "webhook-clear-log",
                            clear.clone(),
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .disabled(disabled || deliveries.is_empty())
                        .on_click(move |_, _, cx| {
                            clear_store.update(cx, |store, cx| {
                                store.call_simple(
                                    "webhookClear",
                                    method::DAEMON_WEBHOOK_CLEAR_DELIVERIES,
                                    json!({}),
                                    None,
                                    cx,
                                );
                            });
                        }),
                    ),
            )
            .into_any_element()
    })
    .keywords([ctx.t("webhookDeliveryLog")]);
    SettingGroup::new()
        .title(ctx.t("webhookDeliveryLog"))
        .description(ctx.t("webhookLogSubtitle"))
        .item(item)
}
