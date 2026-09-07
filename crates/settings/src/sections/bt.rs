//! BitTorrent：基础（DHT/UPnP/端口）、Tracker（列表 + 订阅）、做种。

use gpui::{App, SharedString};
use gpui_component::{
    Icon, IconName,
    setting::{SettingGroup, SettingPage},
};

use super::{SectionContext, subscription};

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingPage {
    SettingPage::new(ctx.t("settingsCatBt"))
        .icon(Icon::new(IconName::HardDrive))
        .description(ctx.t("settingsCatBtDesc"))
        .group(basic_group(ctx))
        .group(tracker_group(ctx, cx))
        .group(seeding_group(ctx, cx))
}

fn basic_group(ctx: &SectionContext) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("settingsTabGeneral"))
        .description(ctx.t("btSettingsRestartHint"))
        .item(ctx.item(
            "btEnableDht",
            Some("btEnableDhtDesc"),
            ctx.daemon_switch("bt_enable_dht"),
        ))
        .item(ctx.item(
            "btEnableUpnp",
            Some("btEnableUpnpDesc"),
            ctx.daemon_switch("bt_enable_upnp"),
        ))
        .item(ctx.item(
            "btListenPortStart",
            Some("btListenPortDesc"),
            ctx.daemon_number("bt_port_start"),
        ))
        .item(ctx.item("btListenPortEnd", None, ctx.daemon_number("bt_port_end")))
        .item(ctx.item(
            "btMseMode",
            Some("btMseModeDesc"),
            ctx.daemon_enum_dropdown("bt_mse_mode", "btMseMode"),
        ))
}

fn tracker_group(ctx: &SectionContext, cx: &mut App) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("settingsTabTracker"))
        .item(subscription::list_item(
            ctx,
            "btTrackerList",
            "btTrackerListDesc",
            "btTrackerPlaceholder",
            "bt_custom_trackers",
        ))
        .item(ctx.item(
            "btTrackerSub",
            Some("btTrackerSubDesc"),
            ctx.daemon_switch("bt_tracker_sub_enabled"),
        ))
        .item(subscription::list_item(
            ctx,
            "btTrackerSub",
            "btTrackerSubDesc",
            "btTrackerSubPlaceholder",
            "bt_tracker_sub_urls",
        ))
        .item(subscription::status_item(
            ctx,
            subscription::SubscriptionKind::BtTrackers,
            cx,
        ))
}

fn seeding_group(ctx: &SectionContext, cx: &mut App) -> SettingGroup {
    let store = ctx.store.read(cx);
    let seed_enabled = store.daemon_bool("bt_seed_enabled");
    let mut group = SettingGroup::new()
        .title(ctx.t("settingsTabSeeding"))
        .item(ctx.item(
            "btSeedEnabled",
            Some("btSeedEnabledDesc"),
            ctx.daemon_switch("bt_seed_enabled"),
        ));
    if !seed_enabled {
        return group;
    }
    group = group
        .item(ctx.item(
            "btSeedMaxActive",
            Some("btSeedMaxActiveDesc"),
            ctx.daemon_number("bt_seed_max_active"),
        ))
        .item(ctx.item(
            "btAutoReseed",
            Some("btAutoReseedDesc"),
            ctx.daemon_switch("bt_auto_reseed"),
        ))
        .item(ctx.item(
            "btSeedRatioLimit",
            None,
            ctx.daemon_number_with("bt_seed_ratio_limit", 0.1),
        ))
        .item(ctx.item(
            "btSeedPostRatioLimit",
            None,
            ctx.daemon_number_with("bt_seed_post_ratio_limit", 0.1),
        ))
        .item(ctx.item(
            "btSeedTimeLimit",
            None,
            ctx.daemon_number("bt_seed_time_limit_minutes"),
        ))
        .item(ctx.item(
            "btSeedTimeLimitUnit",
            None,
            ctx.daemon_dropdown("bt_seed_time_limit_unit", time_units(ctx)),
        ))
        .item(ctx.item(
            "btSeedInactiveTimeLimit",
            None,
            ctx.daemon_number("bt_seed_inactive_time_limit_minutes"),
        ))
        .item(ctx.item(
            "btSeedInactiveTimeLimitUnit",
            None,
            ctx.daemon_dropdown("bt_seed_inactive_time_limit_unit", time_units(ctx)),
        ))
        .item(ctx.item(
            "btSeedConditionsOperator",
            None,
            ctx.daemon_dropdown(
                "bt_seed_limit_operator",
                vec![
                    (SharedString::from("or"), ctx.t("btSeedOperatorOr")),
                    (SharedString::from("and"), ctx.t("btSeedOperatorAnd")),
                ],
            ),
        ))
        .item(ctx.item(
            "btSeedThenAction",
            None,
            ctx.daemon_dropdown(
                "bt_seed_then_action",
                vec![
                    (SharedString::from("stop"), ctx.t("btSeedStopSeeding")),
                    (SharedString::from("delete"), ctx.t("btSeedDeleteTask")),
                    (
                        SharedString::from("delete_files"),
                        ctx.t("btSeedDeleteTaskAndFiles"),
                    ),
                ],
            ),
        ));
    group
}

fn time_units(ctx: &SectionContext) -> Vec<(SharedString, SharedString)> {
    vec![
        (SharedString::from("minutes"), ctx.t("timeUnitMinutes")),
        (SharedString::from("hours"), ctx.t("timeUnitHours")),
        (SharedString::from("days"), ctx.t("timeUnitDays")),
    ]
}
