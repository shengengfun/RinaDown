//! 插件详情对话框：已安装插件与市场条目共用（manifest 级信息 + 权限 + 使用须知）。

use std::rc::Rc;

use fluxdown_protocol::{MarketEntryDto, PluginDto};
use fluxdown_ui_i18n::Translator;
use gpui::{App, IntoElement, ParentElement, SharedString, Styled, Window, div, px};
use gpui_component::{
    ActiveTheme as _, StyledExt as _, WindowExt as _, h_flex, link::Link, tag::Tag, v_flex,
};

/// 详情对话框的数据；调用侧从各自 DTO 拆字段。
#[derive(Clone, Debug, Default)]
pub struct PluginDetail {
    pub name: String,
    pub version: String,
    pub identity: String,
    pub description: String,
    pub homepage: String,
    pub author: String,
    pub tags: Vec<String>,
    pub publish_time: String,
    pub min_app_version: String,
    pub settings_count: usize,
    pub permissions: Vec<String>,
    pub yanked_label: Option<String>,
}

impl PluginDetail {
    pub fn from_plugin(plugin: &PluginDto) -> Self {
        Self {
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            identity: plugin.identity.clone(),
            description: plugin.description.clone(),
            homepage: plugin.homepage.clone(),
            settings_count: plugin.settings.len(),
            permissions: plugin.permissions.clone(),
            ..Self::default()
        }
    }

    pub fn from_market(entry: &MarketEntryDto, translator: &Translator) -> Self {
        Self {
            name: if entry.name.is_empty() {
                entry.plugin_id.clone()
            } else {
                entry.name.clone()
            },
            version: entry.version.clone(),
            identity: entry.plugin_id.clone(),
            description: entry.description.clone(),
            homepage: entry.homepage.clone(),
            author: entry.author.clone(),
            tags: entry.tags.clone(),
            publish_time: entry.publish_time.clone(),
            min_app_version: entry.min_app_version.clone(),
            settings_count: 0,
            permissions: entry.permissions.clone(),
            yanked_label: yanked_label(translator, &entry.yanked),
        }
    }
}

/// 市场 `yanked` 标记 → 文案；空 / 未知值不展示。
pub fn yanked_label(translator: &Translator, yanked: &str) -> Option<String> {
    let key = match yanked {
        "deprecated" => "marketYankedDeprecated",
        "vulnerable" => "marketYankedVulnerable",
        "malicious" => "marketYankedMalicious",
        _ => return None,
    };
    Some(translator.text(key).to_owned())
}

/// 权限 → （展示名，说明）；未知权限降级展示原始名。
pub fn permission_label(translator: &Translator, permission: &str) -> (String, String) {
    match permission {
        "ffmpeg" => (
            translator.text("pluginPermFfmpegName").to_owned(),
            translator.text("pluginPermFfmpegDesc").to_owned(),
        ),
        "ytdlp" => (
            translator.text("pluginPermYtdlpName").to_owned(),
            translator.text("pluginPermYtdlpDesc").to_owned(),
        ),
        other => (
            other.to_owned(),
            translator.text("pluginPermUnknownDesc").to_owned(),
        ),
    }
}

/// 打开详情对话框；对话框构造器每帧重建，内容从共享的只读快照按需渲染。
pub fn open_plugin_detail(
    detail: PluginDetail,
    translator: Translator,
    window: &mut Window,
    cx: &mut App,
) {
    let detail = Rc::new(detail);
    let translator = Rc::new(translator);
    window.open_dialog(cx, move |dialog, _, _| {
        let detail = detail.clone();
        let translator = translator.clone();
        dialog
            .title(detail.name.clone())
            .w(px(480.))
            .content(move |root, _, cx| root.child(render_detail(&detail, &translator, cx)))
    });
}

fn render_detail(detail: &PluginDetail, translator: &Translator, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    let text = |key: &str| SharedString::from(translator.text(key).to_owned());
    let label_cell = |label: SharedString| {
        div()
            .w(px(96.))
            .flex_shrink_0()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(label)
    };
    let info_row = |label: SharedString, value: String| {
        h_flex()
            .w_full()
            .gap_3()
            .items_start()
            .child(label_cell(label))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(value),
            )
    };
    let section_title = |label: SharedString| div().pt_2().text_xs().font_semibold().child(label);
    let permissions = detail
        .permissions
        .iter()
        .map(|permission| {
            let (name, description) = permission_label(translator, permission);
            v_flex()
                .w_full()
                .gap_0p5()
                .child(div().text_xs().font_semibold().child(name))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(description),
                )
        })
        .collect::<Vec<_>>();
    let mut content = v_flex().w_full().gap_2().child(
        h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("v{}", detail.version)),
            )
            .children(
                detail
                    .yanked_label
                    .clone()
                    .map(|label| Tag::danger().child(label)),
            )
            .children(
                detail
                    .tags
                    .iter()
                    .cloned()
                    .map(|tag| Tag::secondary().child(tag)),
            ),
    );
    content = content.child(info_row(
        text("pluginDetailIdentity"),
        detail.identity.clone(),
    ));
    if !detail.author.is_empty() {
        content = content.child(info_row(text("pluginDetailAuthor"), detail.author.clone()));
    }
    if !detail.homepage.is_empty() {
        content = content.child(
            h_flex()
                .w_full()
                .gap_3()
                .items_start()
                .child(label_cell(text("pluginDetailHomepage")))
                .child(
                    Link::new("plugin-detail-homepage")
                        .href(detail.homepage.clone())
                        .text_xs()
                        .child(detail.homepage.clone()),
                ),
        );
    }
    if !detail.publish_time.is_empty() {
        content = content.child(info_row(
            text("pluginDetailPublishTime"),
            detail.publish_time.clone(),
        ));
    }
    if !detail.min_app_version.is_empty() {
        content = content.child(info_row(
            text("pluginDetailMinAppVersion"),
            detail.min_app_version.clone(),
        ));
    }
    if detail.settings_count > 0 {
        content = content.child(info_row(
            text("pluginDetailSettings"),
            translator.text_with(
                "pluginDetailSettingsCount",
                &[("count", &detail.settings_count.to_string())],
            ),
        ));
    }
    if !detail.description.is_empty() {
        content = content
            .child(section_title(text("pluginDetailDescription")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(detail.description.clone()),
            );
    }
    if !permissions.is_empty() {
        content = content
            .child(section_title(text("pluginDetailPermissions")))
            .children(permissions);
    }
    content
        .child(section_title(text("pluginDetailUsage")))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(text("pluginDetailUsageBody")),
        )
}
