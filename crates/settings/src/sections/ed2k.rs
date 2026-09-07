//! eD2K：基础（Kad/UPnP/端口）、服务器（列表 + server.met 订阅）。

use gpui::App;
use gpui_component::{
    Icon, IconName,
    setting::{SettingGroup, SettingPage},
};

use super::{SectionContext, subscription};

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingPage {
    SettingPage::new(ctx.t("settingsCatEd2k"))
        .icon(Icon::new(IconName::HardDrive))
        .description(ctx.t("settingsCatEd2kDesc"))
        .group(
            SettingGroup::new()
                .title(ctx.t("settingsTabGeneral"))
                .item(ctx.item(
                    "ed2kEnableKad",
                    Some("ed2kEnableKadDesc"),
                    ctx.daemon_switch("ed2k_enable_kad"),
                ))
                .item(ctx.item(
                    "ed2kEnableUpnp",
                    Some("ed2kEnableUpnpDesc"),
                    ctx.daemon_switch("ed2k_enable_upnp"),
                ))
                .item(ctx.item(
                    "ed2kListenPort",
                    Some("ed2kListenPortDesc"),
                    ctx.daemon_number("ed2k_listen_port"),
                )),
        )
        .group(
            SettingGroup::new()
                .title(ctx.t("settingsTabServers"))
                .item(subscription::list_item(
                    ctx,
                    "ed2kServerList",
                    "ed2kServerListDesc",
                    "ed2kServerPlaceholder",
                    "ed2k_server_list",
                ))
                .item(ctx.item(
                    "ed2kServerSub",
                    Some("ed2kServerSubDesc"),
                    ctx.daemon_switch("ed2k_server_sub_enabled"),
                ))
                .item(subscription::list_item(
                    ctx,
                    "ed2kServerSub",
                    "ed2kServerSubDesc",
                    "ed2kServerSubPlaceholder",
                    "ed2k_server_sub_urls",
                ))
                .item(subscription::status_item(
                    ctx,
                    subscription::SubscriptionKind::Ed2kServers,
                    cx,
                )),
        )
}
