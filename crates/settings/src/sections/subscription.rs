//! 多行列表编辑（Tracker / 服务器 / 订阅源）与订阅状态行。

use fluxdown_protocol::method;
use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::active_theme;
use gpui::{
    App, AppContext as _, Entity, IntoElement as _, ParentElement, SharedString, Styled,
    Subscription, Window, div, px,
};
use gpui_component::{
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    setting::{SettingField, SettingItem},
    v_flex,
};
use serde_json::json;

use super::SectionContext;
use crate::store::SettingsStore;

struct TextareaSlot {
    state: Entity<TextareaState>,
    last_synced: SharedString,
    _subscription: Subscription,
}

/// 多行文本编辑一个 daemon 文本键；每行一个条目，失焦或改动即写回。
pub(crate) fn list_item(
    ctx: &SectionContext,
    title_key: &str,
    desc_key: &str,
    placeholder_key: &str,
    key: &'static str,
) -> SettingItem {
    let field = textarea_field(ctx.store(), ctx.t(placeholder_key), key);
    ctx.item(title_key, Some(desc_key), field)
        .layout(gpui::Axis::Vertical)
}

pub(crate) fn textarea_field(
    store: Entity<SettingsStore>,
    placeholder: SharedString,
    key: &'static str,
) -> SettingField<SharedString> {
    SettingField::render(move |options, window: &mut Window, cx: &mut App| {
        let current = SharedString::from(store.read(cx).daemon_str(key));
        let slot = window.use_keyed_state(
            SharedString::from(format!("settings-textarea-{key}")),
            cx,
            {
                let store = store.clone();
                let current = current.clone();
                let placeholder = placeholder.clone();
                move |window, cx| {
                    let state = cx.new(|cx| {
                        TextareaState::new(window, cx)
                            .default_value(current.clone())
                            .placeholder(placeholder)
                    });
                    let _subscription = cx.subscribe(
                        &state,
                        move |slot: &mut TextareaSlot, state, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Change | InputEvent::Blur) {
                                let value = state.read(cx).value();
                                if value != slot.last_synced {
                                    slot.last_synced = value.clone();
                                    store.update(cx, |store, cx| {
                                        store.set_daemon(key, value.to_string(), cx)
                                    });
                                }
                            }
                        },
                    );
                    TextareaSlot {
                        state,
                        last_synced: current,
                        _subscription,
                    }
                }
            },
        );
        slot.update(cx, |slot, cx| {
            if slot.last_synced != current {
                slot.last_synced = current.clone();
                slot.state
                    .update(cx, |state, cx| state.set_value(current.clone(), window, cx));
            }
        });
        let state = slot.read(cx).state.clone();
        Textarea::new(&state)
            .h(px(120.))
            .w_full()
            .disabled(options.is_disabled())
            .into_any_element()
    })
}

#[derive(Clone, Copy)]
pub(crate) enum SubscriptionKind {
    BtTrackers,
    Ed2kServers,
}

impl SubscriptionKind {
    fn cache_key(self) -> &'static str {
        match self {
            Self::BtTrackers => "bt_tracker_sub_cache",
            Self::Ed2kServers => "ed2k_server_sub_cache",
        }
    }
    fn updated_at_key(self) -> &'static str {
        match self {
            Self::BtTrackers => "bt_tracker_sub_updated_at",
            Self::Ed2kServers => "ed2k_server_sub_updated_at",
        }
    }
    fn method(self) -> &'static str {
        match self {
            Self::BtTrackers => method::DAEMON_BT_TRACKER_SUBSCRIPTION_REFRESH,
            Self::Ed2kServers => method::DAEMON_ED2K_SERVER_SUBSCRIPTION_REFRESH,
        }
    }
    fn action(self) -> &'static str {
        match self {
            Self::BtTrackers => "btTrackerRefresh",
            Self::Ed2kServers => "ed2kServerRefresh",
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::BtTrackers => "btTrackerSub",
            Self::Ed2kServers => "ed2kServerSub",
        }
    }
}

/// 订阅状态：已订阅条数、更新时间、立即更新。
pub(crate) fn status_item(
    ctx: &SectionContext,
    kind: SubscriptionKind,
    _cx: &mut App,
) -> SettingItem {
    let store = ctx.store();
    let translator = ctx.translator.clone();
    let prefix = kind.prefix();
    let update_now = ctx.t(&format!("{prefix}UpdateNow"));
    let updating = ctx.t(&format!("{prefix}Updating"));
    let failed = ctx.t(&format!("{prefix}UpdateFailed"));
    let never = ctx.t(&format!("{prefix}NeverUpdated"));
    SettingItem::render(move |_, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let snapshot = store.read(cx);
        let count = snapshot
            .daemon_str(kind.cache_key())
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        let updated_at = snapshot.daemon_i64(kind.updated_at_key());
        let busy = snapshot.is_busy(kind.action());
        let last_failed = snapshot
            .transient(kind.action())
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let status = translator.text_with(&format!("{prefix}Status"), &[("n", &count.to_string())]);
        let time = if updated_at > 0 {
            translator.text_with(
                &format!("{prefix}UpdatedAt"),
                &[("time", &format_unix(updated_at))],
            )
        } else {
            never.to_string()
        };
        let click_store = store.clone();
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap(tokens.spacing.md)
            .child(
                v_flex()
                    .gap(tokens.spacing.xxs)
                    .child(div().text_sm().child(SharedString::from(status)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if last_failed {
                                tokens.colors.destructive
                            } else {
                                tokens.colors.muted_foreground
                            })
                            .child(if last_failed {
                                failed.clone()
                            } else {
                                SharedString::from(time)
                            }),
                    ),
            )
            .child(
                button(
                    kind.action(),
                    if busy {
                        updating.clone()
                    } else {
                        update_now.clone()
                    },
                    ButtonVariant::Secondary,
                    cx,
                )
                .disabled(busy)
                .on_click(move |_, _, cx| {
                    click_store.update(cx, |store, cx| {
                        store.call_with(
                            kind.action(),
                            kind.method(),
                            json!({}),
                            cx,
                            move |store, result, cx| {
                                let ok = result
                                    .as_ref()
                                    .ok()
                                    .and_then(|value| value.get("success"))
                                    .and_then(serde_json::Value::as_bool)
                                    .unwrap_or(false);
                                store.set_transient(kind.action(), json!(!ok), cx);
                            },
                        );
                    });
                }),
            )
            .into_any_element()
    })
}

/// Unix 秒 → 本地时间 `YYYY-MM-DD HH:MM`（无时区库，按 UTC 偏移 0 计算）。
pub(crate) fn format_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute) = (rem / 3600, (rem % 3600) / 60);
    // 民用日历转换（Howard Hinnant 算法）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {hour:02}:{minute:02}")
}
