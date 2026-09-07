//! 下载：保存位置、行为、连接与性能、自动重试、高级。

use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_theme::active_theme;
use gpui::{App, ParentElement, SharedString, Styled, div};
use gpui_component::{
    Icon, IconName, h_flex,
    setting::{SettingField, SettingGroup, SettingPage},
};

use super::{SectionContext, user_agent};

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingPage {
    if ctx.store.read(cx).conn_policy().is_none() && !ctx.store.read(cx).is_busy("connPolicy") {
        ctx.store.update(cx, |store, cx| store.load_conn_policy(cx));
    }
    SettingPage::new(ctx.t("settingsCatDownload"))
        .icon(Icon::new(IconName::HardDrive))
        .description(ctx.t("settingsCatDownloadDesc"))
        .group(save_location_group(ctx))
        .group(behavior_group(ctx, cx))
        .group(connection_group(ctx, cx))
        .group(retry_group(ctx))
        .group(advanced_group(ctx))
}

fn save_location_group(ctx: &SectionContext) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("settingsGroupSaveLocation"))
        .item(ctx.item(
            "defaultSaveDir",
            Some("defaultSaveDirDesc"),
            save_dir_field(ctx),
        ))
        .item(ctx.item(
            "rememberLastSaveDir",
            Some("rememberLastSaveDirDesc"),
            ctx.pref_switch("download.remember_last_save_dir", false),
        ))
}

/// 目录选择：文本输入 + 系统目录选择器。
fn save_dir_field(ctx: &SectionContext) -> SettingField<SharedString> {
    let store = ctx.store();
    let browse = ctx.t("browse");
    SettingField::render(move |options, _window, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let current = store.read(cx).daemon_str("default_save_dir");
        let pick_store = store.clone();
        h_flex()
            .gap(tokens.spacing.sm)
            .items_center()
            .child(
                div()
                    .max_w_80()
                    .truncate()
                    .text_sm()
                    .text_color(if current.is_empty() {
                        tokens.colors.muted_foreground
                    } else {
                        tokens.colors.foreground
                    })
                    .child(SharedString::from(current)),
            )
            .child(
                button(
                    "download-pick-save-dir",
                    browse.clone(),
                    ButtonVariant::Secondary,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    let store = pick_store.clone();
                    let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: None,
                    });
                    cx.spawn(async move |cx| {
                        if let Ok(Ok(Some(paths))) = receiver.await
                            && let Some(path) = paths.first()
                        {
                            let text = path.display().to_string();
                            store.update(cx, |store, cx| {
                                store.set_daemon("default_save_dir", text, cx);
                            });
                        }
                    })
                    .detach();
                })
                .disabled(options.is_disabled()),
            )
    })
}

fn behavior_group(ctx: &SectionContext, cx: &mut App) -> SettingGroup {
    let silent = ctx
        .store
        .read(cx)
        .pref_bool("download.silent_download", false);
    let mut group = SettingGroup::new()
        .title(ctx.t("settingsGroupBehavior"))
        .item(ctx.item(
            "silentDownload",
            Some("silentDownloadDesc"),
            ctx.pref_switch("download.silent_download", false),
        ));
    if silent {
        group = group.item(ctx.item(
            "silentSkipSelection",
            Some("silentSkipSelectionDesc"),
            ctx.pref_switch("silent_skip_selection", false),
        ));
    }
    group
        .item(ctx.item(
            "useServerTime",
            Some("useServerTimeDesc"),
            ctx.daemon_switch("use_server_time"),
        ))
        .item(ctx.item(
            "fileExistsBehavior",
            Some("fileExistsBehaviorDesc"),
            ctx.daemon_enum_dropdown("file_exists_behavior", "fileExists"),
        ))
        .item(ctx.item(
            "fileMissingAction",
            Some("fileMissingActionDesc"),
            ctx.daemon_enum_dropdown("file_missing_action", "fileMissing"),
        ))
        .item(ctx.item(
            "defaultQueueSetting",
            Some("defaultQueueSettingDesc"),
            default_queue_field(ctx, cx),
        ))
}

fn default_queue_field(ctx: &SectionContext, cx: &mut App) -> SettingField<SharedString> {
    let mut options = vec![(SharedString::from(""), ctx.t("defaultQueue"))];
    options.extend(ctx.store.read(cx).queues().iter().map(|queue| {
        (
            SharedString::from(queue.queue_id.clone()),
            SharedString::from(queue.name.clone()),
        )
    }));
    ctx.daemon_dropdown("default_queue_id", options)
}

fn connection_group(ctx: &SectionContext, cx: &mut App) -> SettingGroup {
    let store = ctx.store.read(cx);
    let auto_segments = store.daemon_i64("default_segments") == 0;
    let cdn_multi = store.daemon_bool("cdn_multi_enabled");
    let mut group = SettingGroup::new()
        .title(ctx.t("settingsGroupConnection"))
        .item(ctx.item(
            "defaultThreads",
            Some("defaultThreadsDesc"),
            ctx.daemon_number("default_segments"),
        ));
    if auto_segments {
        group = group.item(ctx.item(
            "autoMaxConnections",
            Some("autoMaxConnectionsDesc"),
            ctx.daemon_number("auto_max_connections"),
        ));
    }
    group = group.item(ctx.item(
        "cdnMultiEnabled",
        Some("cdnMultiEnabledDesc"),
        ctx.daemon_switch("cdn_multi_enabled"),
    ));
    if cdn_multi {
        group = group.item(ctx.item(
            "cdnMaxNodes",
            Some("cdnMaxNodesDesc"),
            ctx.daemon_number("cdn_max_nodes"),
        ));
    }
    group
        .item(ctx.item(
            "connPolicyCache",
            Some("connPolicyCacheDesc"),
            conn_policy_field(ctx),
        ))
        .item(ctx.item(
            "maxConcurrent",
            Some("maxConcurrentDesc"),
            ctx.daemon_number("max_concurrent_tasks"),
        ))
        .item(ctx.item(
            "speedLimit",
            Some("speedLimitDesc"),
            bytes_per_second_field(ctx, "speed_limit_bytes"),
        ))
        .item(ctx.item(
            "uploadLimit",
            Some("uploadLimitDesc"),
            bytes_per_second_field(ctx, "upload_limit_bytes"),
        ))
}

/// 以 KB/s 显示与编辑字节速率键；0 = 不限。
fn bytes_per_second_field(ctx: &SectionContext, key: &'static str) -> SettingField<f64> {
    let get = ctx.store();
    let set = ctx.store();
    SettingField::number_input(
        gpui_component::setting::NumberFieldOptions {
            min: 0.0,
            max: (i64::MAX / 1024) as f64,
            step: 64.0,
        },
        move |cx: &App| (get.read(cx).daemon_i64(key) / 1024) as f64,
        move |value, cx: &mut App| {
            set.update(cx, |store, cx| {
                store.set_daemon_i64(key, (value.round() as i64).saturating_mul(1024), cx);
            });
        },
    )
    .default_value(0.0)
}

fn conn_policy_field(ctx: &SectionContext) -> SettingField<SharedString> {
    let store = ctx.store();
    let clear = ctx.t("connPolicyCacheClear");
    let empty = ctx.t("connPolicyCacheEmpty");
    SettingField::render(move |_, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let count = store
            .read(cx)
            .conn_policy()
            .map_or(0, |summary| summary.domain_count);
        let busy = store.read(cx).is_busy("connPolicy");
        let clear_store = store.clone();
        h_flex()
            .gap(tokens.spacing.sm)
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(if count == 0 {
                        empty.clone()
                    } else {
                        SharedString::from(count.to_string())
                    }),
            )
            .child(
                button(
                    "download-clear-conn-policy",
                    clear.clone(),
                    ButtonVariant::Secondary,
                    cx,
                )
                .disabled(busy || count == 0)
                .on_click(move |_, _, cx| {
                    clear_store.update(cx, |store, cx| store.clear_conn_policy(cx));
                }),
            )
    })
}

fn retry_group(ctx: &SectionContext) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("settingsGroupRetry"))
        .item(ctx.item(
            "autoRetryCount",
            Some("autoRetryCountDesc"),
            ctx.daemon_number("max_auto_retries"),
        ))
        .item(ctx.item(
            "autoRetryDelay",
            Some("autoRetryDelayDesc"),
            ctx.daemon_number("auto_retry_delay_secs"),
        ))
        .item(ctx.item(
            "autoResumeOnStart",
            Some("autoResumeOnStartDesc"),
            ctx.daemon_switch("auto_resume_on_start"),
        ))
}

fn advanced_group(ctx: &SectionContext) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("settingsGroupAdvanced"))
        .item(ctx.item("userAgent", Some("userAgentDesc"), user_agent::field(ctx)))
        .item(ctx.item(
            "revealFileCmdLabel",
            Some("revealFileCmdDesc"),
            ctx.pref_input("reveal_file_cmd", ""),
        ))
}
