//! 全局 User-Agent：预设下拉 + 自定义输入，与 `lib/src/models/ua_presets.dart` 同基线。

use fluxdown_ui_theme::active_theme;
use gpui::{
    App, AppContext as _, Entity, IntoElement as _, ParentElement, SharedString, Styled,
    Subscription, Window, px,
};
use gpui_component::{
    input::{Input, InputEvent, InputState},
    setting::SettingField,
    v_flex,
};

use super::SectionContext;

pub(crate) const UA_KEY: &str = "global_user_agent";

/// 预设 UA（key → UA 字符串）。版本基准与 Dart 侧一致。
pub(crate) const UA_PRESETS: &[(&str, &str)] = &[
    (
        "chrome",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36",
    ),
    (
        "firefox",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:147.0) Gecko/20100101 Firefox/147.0",
    ),
    (
        "edge",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.3800.70",
    ),
    (
        "safari",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Safari/605.1.15",
    ),
];

/// UA 字符串 → 预设键；空 = `default`，未命中 = `custom`。
#[must_use]
pub(crate) fn detect_preset(ua: &str) -> &'static str {
    if ua.is_empty() {
        return "default";
    }
    UA_PRESETS
        .iter()
        .find(|(_, value)| *value == ua)
        .map_or("custom", |(key, _)| key)
}

struct CustomSlot {
    input: Entity<InputState>,
    last_synced: SharedString,
    _subscription: Subscription,
}

pub(crate) fn field(ctx: &SectionContext) -> SettingField<SharedString> {
    let store = ctx.store();
    let options: Vec<(SharedString, SharedString)> = vec![
        (
            SharedString::from("default"),
            ctx.t("userAgentPresetDefault"),
        ),
        (SharedString::from("chrome"), ctx.t("userAgentPresetChrome")),
        (
            SharedString::from("firefox"),
            ctx.t("userAgentPresetFirefox"),
        ),
        (SharedString::from("edge"), ctx.t("userAgentPresetEdge")),
        (SharedString::from("safari"), ctx.t("userAgentPresetSafari")),
        (SharedString::from("custom"), ctx.t("userAgentPresetCustom")),
    ];
    let placeholder = ctx.t("userAgentPlaceholder");
    SettingField::render(move |render_options, window: &mut Window, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let current = store.read(cx).daemon_str(UA_KEY);
        let preset = detect_preset(&current);
        let custom_active = store
            .read(cx)
            .transient("ua_custom_mode")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            || preset == "custom";

        // 预设用一组互斥按钮表达；选择自定义时展开输入框。
        let buttons = gpui_component::h_flex()
            .gap(tokens.spacing.xs)
            .flex_wrap()
            .children(options.iter().map(|(value, label)| {
                let selected = if custom_active {
                    value.as_ref() == "custom"
                } else {
                    value.as_ref() == preset
                };
                let value_for_click = value.clone();
                let click_store = store.clone();
                fluxdown_ui_components::button(
                    SharedString::from(format!("ua-preset-{value}")),
                    label.clone(),
                    if selected {
                        fluxdown_ui_components::ButtonVariant::Primary
                    } else {
                        fluxdown_ui_components::ButtonVariant::Secondary
                    },
                    cx,
                )
                .disabled(render_options.is_disabled())
                .on_click(move |_, _, cx| {
                    click_store.update(cx, |store, cx| match value_for_click.as_ref() {
                        "default" => {
                            store.set_transient("ua_custom_mode", serde_json::json!(false), cx);
                            store.set_daemon(UA_KEY, "", cx);
                        }
                        "custom" => {
                            store.set_transient("ua_custom_mode", serde_json::json!(true), cx);
                        }
                        key => {
                            store.set_transient("ua_custom_mode", serde_json::json!(false), cx);
                            if let Some((_, ua)) = UA_PRESETS.iter().find(|(k, _)| *k == key) {
                                store.set_daemon(UA_KEY, *ua, cx);
                            }
                        }
                    });
                })
            }));

        let mut column = v_flex()
            .w_full()
            .gap(tokens.spacing.sm)
            .items_end()
            .child(buttons);
        if custom_active {
            let slot = window.use_keyed_state(SharedString::from("settings-ua-custom"), cx, {
                let store = store.clone();
                let current = SharedString::from(current.clone());
                let placeholder = placeholder.clone();
                move |window, cx| {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(current.clone())
                            .placeholder(placeholder)
                    });
                    let _subscription = cx.subscribe(
                        &input,
                        move |slot: &mut CustomSlot, input, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Change | InputEvent::Blur) {
                                let value = input.read(cx).value();
                                if value != slot.last_synced {
                                    slot.last_synced = value.clone();
                                    store.update(cx, |store, cx| {
                                        store.set_daemon(UA_KEY, value.to_string(), cx)
                                    });
                                }
                            }
                        },
                    );
                    CustomSlot {
                        input,
                        last_synced: current,
                        _subscription,
                    }
                }
            });
            let current = SharedString::from(current);
            slot.update(cx, |slot, cx| {
                if slot.last_synced != current {
                    slot.last_synced = current.clone();
                    slot.input
                        .update(cx, |input, cx| input.set_value(current.clone(), window, cx));
                }
            });
            let input = slot.read(cx).input.clone();
            column = column.child(
                Input::new(&input)
                    .w(px(420.))
                    .disabled(render_options.is_disabled()),
            );
        }
        column.into_any_element()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_presets() {
        assert_eq!(detect_preset(""), "default");
        assert_eq!(detect_preset(UA_PRESETS[0].1), "chrome");
        assert_eq!(detect_preset("Custom/1.0"), "custom");
    }
}
