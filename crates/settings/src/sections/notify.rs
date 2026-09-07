//! 通知：系统通知开关 + Webhook 端点与投递记录。

use gpui::App;
use gpui_component::{
    Icon, IconName,
    setting::{SettingGroup, SettingPage},
};

use super::{SectionContext, webhook};

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingPage {
    SettingPage::new(ctx.t("settingsCatNotify"))
        .icon(Icon::new(IconName::Bell))
        .description(ctx.t("settingsCatNotifyDesc"))
        .group(
            SettingGroup::new()
                .title(ctx.t("notifyGroupSystem"))
                .item(ctx.item(
                    "notifyOnComplete",
                    Some("notifyOnCompleteDesc"),
                    ctx.pref_switch("download.notify_on_complete", true),
                )),
        )
        .group(webhook::endpoints_group(ctx, cx))
        .group(webhook::delivery_log_group(ctx, cx))
}
