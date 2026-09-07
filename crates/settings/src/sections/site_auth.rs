//! 已保存的站点 HTTP Basic 凭据管理（只列站点与用户名，可逐条删除或清空）。

use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::active_theme;
use gpui::{App, IntoElement as _, ParentElement, SharedString, Styled, div};
use gpui_component::{
    h_flex,
    setting::{SettingGroup, SettingItem},
    v_flex,
};

use super::SectionContext;

pub(crate) fn group(ctx: &SectionContext, cx: &mut App) -> SettingGroup {
    if ctx.store.read(cx).site_auth().is_empty()
        && ctx.store.read(cx).transient("site_auth_loaded").is_none()
    {
        ctx.store.update(cx, |store, cx| {
            store.set_transient("site_auth_loaded", serde_json::json!(true), cx);
            store.load_site_auth(cx);
        });
    }
    SettingGroup::new()
        .title(ctx.t("settingsSiteAuthTitle"))
        .description(ctx.t("settingsSiteAuthDesc"))
        .item(list_item(ctx))
}

fn list_item(ctx: &SectionContext) -> SettingItem {
    let store = ctx.store();
    let empty = ctx.t("settingsSiteAuthEmpty");
    let delete = ctx.t("settingsSiteAuthDelete");
    let clear_all = ctx.t("settingsSiteAuthClearAll");
    SettingItem::render(move |_, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let entries = store.read(cx).site_auth().to_vec();
        let busy = store.read(cx).is_busy("siteAuth");
        let clear_store = store.clone();
        let mut column = v_flex().w_full().gap(tokens.spacing.xs);
        if entries.is_empty() {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(tokens.colors.muted_foreground)
                    .child(empty.clone()),
            );
        }
        for entry in entries {
            let site = entry.site.clone();
            let delete_store = store.clone();
            column = column.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap(tokens.spacing.md)
                    .py(tokens.spacing.xs)
                    .border_b_1()
                    .border_color(tokens.colors.border)
                    .child(
                        v_flex()
                            .gap(tokens.spacing.xxs)
                            .child(
                                div()
                                    .text_sm()
                                    .child(SharedString::from(entry.site.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(SharedString::from(entry.user.clone())),
                            ),
                    )
                    .child(
                        button(
                            SharedString::from(format!("site-auth-delete-{}", entry.site)),
                            delete.clone(),
                            ButtonVariant::Destructive,
                            cx,
                        )
                        .disabled(busy)
                        .on_click(move |_, _, cx| {
                            let site = site.clone();
                            delete_store.update(cx, |store, cx| store.delete_site_auth(&site, cx));
                        }),
                    ),
            );
        }
        column
            .child(
                h_flex().w_full().justify_end().child(
                    button(
                        "site-auth-clear",
                        clear_all.clone(),
                        ButtonVariant::Secondary,
                        cx,
                    )
                    .disabled(busy || store.read(cx).site_auth().is_empty())
                    .on_click(move |_, _, cx| {
                        clear_store.update(cx, |store, cx| store.clear_site_auth(cx));
                    }),
                ),
            )
            .into_any_element()
    })
    .keywords([ctx.t("settingsSiteAuthTitle")])
}
