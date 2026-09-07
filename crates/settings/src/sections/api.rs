//! API 服务：本机网关的功能开关、端口、访问令牌与 LAN 暴露。

use fluxdown_protocol::GatewayPatchParams;
use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::active_theme;
use gpui::{
    App, AppContext as _, ClipboardItem, Entity, ParentElement, SharedString, Styled, Window, div,
    px,
};
use gpui_component::{
    Icon, IconName, h_flex,
    input::{Input, InputEvent, InputState},
    setting::{SettingField, SettingGroup, SettingPage},
    v_flex,
};

use super::SectionContext;

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingPage {
    SettingPage::new(ctx.t("settingsCatApiService"))
        .icon(Icon::new(IconName::Globe))
        .description(ctx.t("settingsCatApiServiceDesc"))
        .resettable(false)
        .group(service_group(ctx, cx))
        .group(features_group(ctx))
}

fn service_group(ctx: &SectionContext, cx: &mut App) -> SettingGroup {
    let gateway = ctx.store.read(cx).gateway().clone();
    let port_text = SharedString::from(gateway.port.to_string());
    let address = SharedString::from(format!("http://127.0.0.1:{}", gateway.port));
    let address_for_copy = address.clone();
    let copied = ctx.t("apiServiceCopied");
    let copy_label = ctx.t("apiServiceCopy");

    SettingGroup::new()
        .title(ctx.t("settingsCatApiService"))
        .item(ctx.item(
            "apiServicePort",
            Some("apiServicePortDesc"),
            SettingField::render(move |_, _, _| div().child(port_text.clone())),
        ))
        .item(
            ctx.item(
                "apiServiceAddress",
                None,
                SettingField::render(move |_, _, cx: &mut App| {
                    let tokens = active_theme(cx).tokens();
                    let address = address_for_copy.clone();
                    h_flex()
                        .gap(tokens.spacing.sm)
                        .items_center()
                        .child(div().text_sm().child(address.clone()))
                        .child(
                            button(
                                "api-copy-address",
                                copy_label.clone(),
                                ButtonVariant::Secondary,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    address.to_string(),
                                ));
                            }),
                        )
                }),
            )
            .keywords([copied.clone()]),
        )
        .item(ctx.item(
            "apiServiceLanEnable",
            Some("apiServiceLanEnableDesc"),
            gateway_switch(ctx, GatewayFlag::Lan),
        ))
        .item(ctx.item(
            "apiServiceToken",
            Some("apiServiceTokenDesc"),
            token_field(ctx),
        ))
}

fn features_group(ctx: &SectionContext) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("apiServiceFeaturesTitle"))
        .description(ctx.t("apiServiceFeaturesDesc"))
        .item(ctx.item(
            "apiServiceTakeover",
            Some("apiServiceTakeoverDesc"),
            gateway_switch(ctx, GatewayFlag::Takeover),
        ))
        .item(ctx.item(
            "apiServiceJsonrpc",
            Some("apiServiceJsonrpcDesc"),
            gateway_switch(ctx, GatewayFlag::Jsonrpc),
        ))
        .item(ctx.item(
            "apiServiceApi",
            Some("apiServiceApiDesc"),
            gateway_switch(ctx, GatewayFlag::Api),
        ))
        .item(ctx.item(
            "apiServiceMcp",
            Some("apiServiceMcpDesc"),
            gateway_switch(ctx, GatewayFlag::Mcp),
        ))
        .item(ctx.item(
            "apiServiceCorsAllowAll",
            Some("apiServiceCorsAllowAllDesc"),
            gateway_switch(ctx, GatewayFlag::Cors),
        ))
}

#[derive(Clone, Copy)]
enum GatewayFlag {
    Takeover,
    Jsonrpc,
    Api,
    Mcp,
    Cors,
    Lan,
}

fn gateway_switch(ctx: &SectionContext, flag: GatewayFlag) -> SettingField<bool> {
    let get = ctx.store();
    let set = ctx.store();
    SettingField::switch(
        move |cx: &App| {
            let gateway = get.read(cx).gateway();
            match flag {
                GatewayFlag::Takeover => gateway.takeover_enabled,
                GatewayFlag::Jsonrpc => gateway.jsonrpc_enabled,
                GatewayFlag::Api => gateway.api_enabled,
                GatewayFlag::Mcp => gateway.mcp_enabled,
                GatewayFlag::Cors => gateway.cors_enabled,
                GatewayFlag::Lan => gateway.lan_enabled,
            }
        },
        move |value, cx: &mut App| {
            let mut patch = GatewayPatchParams::default();
            match flag {
                GatewayFlag::Takeover => patch.takeover_enabled = Some(value),
                GatewayFlag::Jsonrpc => patch.jsonrpc_enabled = Some(value),
                GatewayFlag::Api => patch.api_enabled = Some(value),
                GatewayFlag::Mcp => patch.mcp_enabled = Some(value),
                GatewayFlag::Cors => patch.cors_enabled = Some(value),
                GatewayFlag::Lan => patch.lan_enabled = Some(value),
            }
            set.update(cx, |store, cx| store.patch_gateway(patch, cx));
        },
    )
}

struct TokenSlot {
    input: Entity<InputState>,
    last_synced: SharedString,
    _subscription: gpui::Subscription,
}

/// 令牌：可见可编辑（回车/失焦提交自定义值）+ 复制 / 生成 / 清空。
/// 文本经 `agent.gateway.revealToken` 按需读取，不走快照。
fn token_field(ctx: &SectionContext) -> SettingField<SharedString> {
    let store = ctx.store();
    let generate = ctx.t("apiServiceTokenGenerate");
    let clear = ctx.t("apiServiceTokenClear");
    let copy = ctx.t("apiServiceCopy");
    let copied = ctx.t("apiServiceCopied");
    let placeholder = ctx.t("proxyNotConfigured");
    SettingField::render(move |options, window: &mut Window, cx: &mut App| {
        let tokens = active_theme(cx).tokens().clone();
        let snapshot = store.read(cx);
        let token: SharedString = snapshot
            .transient("gateway_user_token")
            .and_then(serde_json::Value::as_str)
            .map(|token| SharedString::from(token.to_owned()))
            .unwrap_or_default();
        let revealed = snapshot.transient("gateway_user_token").is_some();
        let busy = snapshot.is_busy("gateway") || snapshot.is_busy("gatewayToken");
        let just_copied = snapshot
            .transient("gateway_token_copied")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !revealed && !busy {
            store.update(cx, |store, cx| store.reveal_gateway_token(cx));
        }

        let slot = window.use_keyed_state(SharedString::from("settings-api-token"), cx, {
            let store = store.clone();
            let token = token.clone();
            let placeholder = placeholder.clone();
            move |window, cx| {
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(token.clone())
                        .placeholder(placeholder)
                });
                let _subscription = cx.subscribe(
                    &input,
                    move |slot: &mut TokenSlot, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                            let value =
                                SharedString::from(input.read(cx).value().trim().to_owned());
                            if value != slot.last_synced {
                                slot.last_synced = value.clone();
                                store.update(cx, |store, cx| {
                                    store.patch_gateway(
                                        GatewayPatchParams {
                                            user_token: Some(value.to_string()),
                                            ..Default::default()
                                        },
                                        cx,
                                    );
                                });
                            }
                        }
                    },
                );
                TokenSlot {
                    input,
                    last_synced: token,
                    _subscription,
                }
            }
        });
        slot.update(cx, |slot, cx| {
            if slot.last_synced != token {
                slot.last_synced = token.clone();
                slot.input
                    .update(cx, |input, cx| input.set_value(token.clone(), window, cx));
            }
        });
        let input = slot.read(cx).input.clone();

        let copy_store = store.clone();
        let copy_token = token.clone();
        let generate_store = store.clone();
        let clear_store = store.clone();
        v_flex()
            .w(px(360.))
            .max_w_full()
            .gap(tokens.spacing.xs)
            .items_end()
            .child(
                Input::new(&input)
                    .w_full()
                    .disabled(options.is_disabled() || busy),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(tokens.spacing.sm)
                    .child(
                        button(
                            "api-token-copy",
                            if just_copied {
                                copied.clone()
                            } else {
                                copy.clone()
                            },
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .disabled(token.is_empty())
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                copy_token.to_string(),
                            ));
                            copy_store.update(cx, |store, cx| {
                                store.set_transient(
                                    "gateway_token_copied",
                                    serde_json::json!(true),
                                    cx,
                                );
                            });
                            let reset = copy_store.clone();
                            cx.spawn(async move |cx| {
                                cx.background_executor()
                                    .timer(std::time::Duration::from_secs(2))
                                    .await;
                                reset.update(cx, |store, cx| {
                                    store.set_transient(
                                        "gateway_token_copied",
                                        serde_json::json!(false),
                                        cx,
                                    );
                                });
                            })
                            .detach();
                        }),
                    )
                    .child(
                        button(
                            "api-token-generate",
                            generate.clone(),
                            ButtonVariant::Primary,
                            cx,
                        )
                        .disabled(options.is_disabled() || busy)
                        .on_click(move |_, _, cx| {
                            generate_store.update(cx, |store, cx| {
                                store.patch_gateway(
                                    GatewayPatchParams {
                                        regenerate_user_token: true,
                                        ..Default::default()
                                    },
                                    cx,
                                );
                            });
                        }),
                    )
                    .child(
                        button(
                            "api-token-clear",
                            clear.clone(),
                            ButtonVariant::Destructive,
                            cx,
                        )
                        .disabled(options.is_disabled() || busy || token.is_empty())
                        .on_click(move |_, _, cx| {
                            clear_store.update(cx, |store, cx| {
                                store.patch_gateway(
                                    GatewayPatchParams {
                                        user_token: Some(String::new()),
                                        ..Default::default()
                                    },
                                    cx,
                                );
                            });
                        }),
                    ),
            )
    })
}
