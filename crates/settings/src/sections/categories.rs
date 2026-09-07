//! 自定义分类：模型（与 `lib/src/models/custom_category.dart` 同 JSON 形状）与列表分区。

use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{App, Context, IntoElement as _, ParentElement, SharedString, Styled, div, px};
use gpui_component::{
    Icon, h_flex,
    setting::{SettingGroup, SettingItem},
    v_flex,
};
use serde::{Deserialize, Serialize};

use super::{SectionContext, category_dialog};
use crate::store::SettingsStore;

pub(crate) const CATEGORIES_KEY: &str = "custom_categories";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CategoryEntry {
    pub id: String,
    pub name: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default = "default_match_mode")]
    pub match_mode: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub regex_pattern: String,
    #[serde(default)]
    pub position: i64,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub is_builtin: bool,
    #[serde(default)]
    pub builtin_type: Option<String>,
    #[serde(default)]
    pub save_dir: String,
}

fn default_icon() -> String {
    "file".to_owned()
}
fn default_match_mode() -> String {
    "extension".to_owned()
}
fn default_true() -> bool {
    true
}

/// 内置分类基线（与 Flutter `CustomCategory.builtinDefaults` 同序同扩展名）。
pub(crate) fn builtin_defaults() -> Vec<CategoryEntry> {
    let make = |id: &str, icon: &str, exts: &[&str], position: i64| CategoryEntry {
        id: format!("builtin_{id}"),
        name: String::new(),
        icon: icon.to_owned(),
        match_mode: "extension".to_owned(),
        extensions: exts.iter().map(|ext| (*ext).to_owned()).collect(),
        regex_pattern: String::new(),
        position,
        visible: true,
        is_builtin: true,
        builtin_type: Some(id.to_owned()),
        save_dir: String::new(),
    };
    vec![
        make("all", "folders", &[], 0),
        make(
            "video",
            "film",
            &[
                "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts", "m3u8",
            ],
            1,
        ),
        make(
            "audio",
            "music",
            &["mp3", "flac", "wav", "aac", "ogg", "m4a", "wma", "opus"],
            2,
        ),
        make(
            "document",
            "fileText",
            &[
                "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "epub", "md",
            ],
            3,
        ),
        make(
            "image",
            "image",
            &[
                "jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "heic", "avif",
            ],
            4,
        ),
        make(
            "program",
            "cpu",
            &["exe", "msi", "dmg", "pkg", "deb", "rpm", "apk", "appimage"],
            5,
        ),
        make(
            "archive",
            "archive",
            &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso"],
            6,
        ),
        make("other", "file", &[], 7),
    ]
}

/// 读取分类列表；未设置或损坏时回退内置基线。
pub(crate) fn read_categories(store: &SettingsStore) -> Vec<CategoryEntry> {
    let parsed = match store.pref(CATEGORIES_KEY) {
        Some(serde_json::Value::String(text)) => {
            serde_json::from_str::<Vec<CategoryEntry>>(text).ok()
        }
        Some(value) => serde_json::from_value::<Vec<CategoryEntry>>(value.clone()).ok(),
        None => None,
    };
    let mut list = parsed
        .filter(|list| !list.is_empty())
        .unwrap_or_else(builtin_defaults);
    list.sort_by_key(|entry| entry.position);
    list
}

pub(crate) fn write_categories(
    store: &mut SettingsStore,
    mut list: Vec<CategoryEntry>,
    cx: &mut Context<SettingsStore>,
) {
    for (index, entry) in list.iter_mut().enumerate() {
        entry.position = index as i64;
    }
    let value = serde_json::to_value(&list).unwrap_or(serde_json::Value::Array(Vec::new()));
    store.set_pref(CATEGORIES_KEY, value, cx);
}

/// 内置分类的显示名文案键。
pub(crate) fn builtin_label_key(builtin_type: &str) -> &'static str {
    match builtin_type {
        "all" => "categoryAll",
        "video" => "categoryVideo",
        "audio" => "categoryAudio",
        "document" => "categoryDocument",
        "image" => "categoryImage",
        "program" => "categoryProgram",
        "archive" => "categoryArchive",
        _ => "categoryOther",
    }
}

pub(crate) fn display_name(translator: &Translator, entry: &CategoryEntry) -> SharedString {
    match entry.builtin_type.as_deref() {
        Some(kind) if entry.is_builtin => {
            SharedString::from(translator.text(builtin_label_key(kind)).to_owned())
        }
        _ => SharedString::from(entry.name.clone()),
    }
}

pub(crate) fn group(ctx: &SectionContext, _cx: &mut App) -> SettingGroup {
    SettingGroup::new()
        .title(ctx.t("customCategories"))
        .description(ctx.t("categoryPriorityNote"))
        .item(list_item(ctx))
}

fn list_item(ctx: &SectionContext) -> SettingItem {
    let store = ctx.store();
    let translator = ctx.translator.clone();
    let builtin = ctx.t("builtinCategory");
    let custom = ctx.t("customCategory");
    let move_up = ctx.t("moveUpAction");
    let move_down = ctx.t("moveDownAction");
    let edit = ctx.t("editCategory");
    let delete = ctx.t("delete");
    let add = ctx.t("addCategory");
    let reset = ctx.t("resetBuiltinCategories");
    let auto_dirs = ctx.t("autoCategoryDirs");
    let regex_label = ctx.t("regexLabel");
    SettingItem::render(move |options, _, cx: &mut App| {
        let tokens = active_theme(cx).tokens();
        let disabled = options.is_disabled();
        let list = read_categories(store.read(cx));
        let count = list.len();
        let mut column = v_flex().w_full().gap(tokens.spacing.xs);
        for (index, entry) in list.iter().enumerate() {
            let name = display_name(&translator, entry);
            let details = if entry.match_mode == "regex" {
                format!("{regex_label}: {}", entry.regex_pattern)
            } else if entry.extensions.is_empty() {
                String::new()
            } else {
                entry
                    .extensions
                    .iter()
                    .map(|ext| format!(".{ext}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let save_dir = entry.save_dir.clone();
            let up_store = store.clone();
            let down_store = store.clone();
            let edit_store = store.clone();
            let edit_translator = translator.clone();
            let edit_entry = entry.clone();
            let delete_store = store.clone();
            let id_up = entry.id.clone();
            let id_down = entry.id.clone();
            let id_delete = entry.id.clone();
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
                        Icon::new(category_dialog::icon_for(&entry.icon))
                            .size(px(16.))
                            .text_color(tokens.colors.muted_foreground),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(tokens.spacing.xxs)
                            .child(
                                h_flex()
                                    .gap(tokens.spacing.sm)
                                    .items_center()
                                    .child(div().text_sm().child(name))
                                    .child(
                                        div()
                                            .px(tokens.spacing.xs)
                                            .rounded(tokens.radius.sm)
                                            .bg(tokens.colors.accent)
                                            .text_color(tokens.colors.accent_foreground)
                                            .text_xs()
                                            .child(if entry.is_builtin {
                                                builtin.clone()
                                            } else {
                                                custom.clone()
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(SharedString::from(details)),
                            )
                            .when_some_dir(save_dir, tokens.colors.muted_foreground),
                    )
                    .child(
                        button(
                            SharedString::from(format!("category-up-{}", entry.id)),
                            move_up.clone(),
                            ButtonVariant::Ghost,
                            cx,
                        )
                        .disabled(disabled || index == 0)
                        .on_click(move |_, _, cx| {
                            let id = id_up.clone();
                            up_store.update(cx, |store, cx| move_category(store, &id, -1, cx));
                        }),
                    )
                    .child(
                        button(
                            SharedString::from(format!("category-down-{}", entry.id)),
                            move_down.clone(),
                            ButtonVariant::Ghost,
                            cx,
                        )
                        .disabled(disabled || index + 1 == count)
                        .on_click(move |_, _, cx| {
                            let id = id_down.clone();
                            down_store.update(cx, |store, cx| move_category(store, &id, 1, cx));
                        }),
                    )
                    .child(
                        button(
                            SharedString::from(format!("category-edit-{}", entry.id)),
                            edit.clone(),
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .disabled(disabled)
                        .on_click(move |_, window, cx| {
                            category_dialog::open(
                                edit_store.clone(),
                                edit_translator.clone(),
                                Some(edit_entry.clone()),
                                window,
                                cx,
                            );
                        }),
                    )
                    .child(
                        button(
                            SharedString::from(format!("category-delete-{}", entry.id)),
                            delete.clone(),
                            ButtonVariant::Destructive,
                            cx,
                        )
                        .disabled(disabled || entry.is_builtin)
                        .on_click(move |_, _, cx| {
                            let id = id_delete.clone();
                            delete_store.update(cx, |store, cx| {
                                let list = read_categories(store)
                                    .into_iter()
                                    .filter(|entry| entry.id != id)
                                    .collect();
                                write_categories(store, list, cx);
                            });
                        }),
                    ),
            );
        }
        let reset_store = store.clone();
        let auto_store = store.clone();
        let add_store = store.clone();
        let add_translator = translator.clone();
        column
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(tokens.spacing.sm)
                    .child(
                        button("category-add", add.clone(), ButtonVariant::Primary, cx)
                            .disabled(disabled)
                            .on_click(move |_, window, cx| {
                                category_dialog::open(
                                    add_store.clone(),
                                    add_translator.clone(),
                                    None,
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .child(
                        button(
                            "category-auto-dirs",
                            auto_dirs.clone(),
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .disabled(disabled)
                        .on_click(move |_, _, cx| {
                            auto_store.update(cx, apply_auto_dirs);
                        }),
                    )
                    .child(
                        button(
                            "category-reset",
                            reset.clone(),
                            ButtonVariant::Secondary,
                            cx,
                        )
                        .disabled(disabled)
                        .on_click(move |_, _, cx| {
                            reset_store.update(cx, |store, cx| {
                                write_categories(store, builtin_defaults(), cx);
                            });
                        }),
                    ),
            )
            .into_any_element()
    })
    .keywords([ctx.t("customCategories"), ctx.t("customCategory")])
}

trait DirLine {
    fn when_some_dir(self, dir: String, color: gpui::Hsla) -> Self;
}

impl DirLine for gpui::Div {
    fn when_some_dir(self, dir: String, color: gpui::Hsla) -> Self {
        if dir.is_empty() {
            self
        } else {
            self.child(
                div()
                    .truncate()
                    .text_xs()
                    .text_color(color)
                    .child(SharedString::from(dir)),
            )
        }
    }
}

pub(crate) fn move_category(
    store: &mut SettingsStore,
    id: &str,
    delta: isize,
    cx: &mut Context<SettingsStore>,
) {
    let mut list = read_categories(store);
    let Some(index) = list.iter().position(|entry| entry.id == id) else {
        return;
    };
    let target = index as isize + delta;
    if target < 0 || target as usize >= list.len() {
        return;
    }
    list.swap(index, target as usize);
    write_categories(store, list, cx);
}

/// 「一键分类目录」：把每个分类的保存目录设为默认下载目录下的同名子目录。
/// 目录名净化规则与 `sanitizeCategoryDirName` 一致。
pub(crate) fn apply_auto_dirs(store: &mut SettingsStore, cx: &mut Context<SettingsStore>) {
    let base = store.daemon_str("default_save_dir");
    if base.trim().is_empty() {
        return;
    }
    let mut list = read_categories(store);
    for entry in &mut list {
        if entry.builtin_type.as_deref() == Some("all") {
            continue;
        }
        let label = match entry.builtin_type.as_deref() {
            Some(kind) if entry.is_builtin => builtin_dir_label(kind).to_owned(),
            _ => entry.name.clone(),
        };
        entry.save_dir = category_dir_under(&base, &label);
    }
    write_categories(store, list, cx);
}

/// 内置分类的目录名基线（英文，与 App `assets/i18n/en.json` 的 `categoryXxx` 逐字一致）。
fn builtin_dir_label(kind: &str) -> &'static str {
    match kind {
        "video" => "Video",
        "audio" => "Audio",
        "document" => "Document",
        "image" => "Image",
        "program" => "Programs",
        "archive" => "Archive",
        _ => "Other",
    }
}

#[must_use]
pub(crate) fn sanitize_category_dir_name(label: &str) -> String {
    let mut out: String = label
        .chars()
        .map(|ch| {
            if matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_control()
            {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    out
}

#[must_use]
pub(crate) fn category_dir_under(base_dir: &str, label: &str) -> String {
    let mut root = base_dir.trim().to_owned();
    if root.is_empty() {
        return String::new();
    }
    let folder = sanitize_category_dir_name(label);
    if folder.is_empty() {
        return String::new();
    }
    while root.len() > 1 && (root.ends_with('/') || root.ends_with('\\')) {
        root.pop();
    }
    if root.ends_with('/') || root.ends_with('\\') {
        return format!("{root}{folder}");
    }
    format!("{root}{}{folder}", std::path::MAIN_SEPARATOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_dir_names_like_dart() {
        assert_eq!(sanitize_category_dir_name("a/b:c "), "a b c");
        assert_eq!(sanitize_category_dir_name("name..."), "name");
        assert_eq!(sanitize_category_dir_name("  "), "");
    }

    #[test]
    fn joins_under_base() {
        assert_eq!(category_dir_under("", "Video"), "");
        assert_eq!(category_dir_under("/", "Video"), "/Video");
        assert_eq!(
            category_dir_under("/tmp/dl/", "Video"),
            format!("/tmp/dl{}Video", std::path::MAIN_SEPARATOR)
        );
    }

    #[test]
    fn json_shape_matches_dart() {
        let json = r#"[{"id":"x","name":"eBooks","icon":"library","matchMode":"extension","extensions":["epub"],"regexPattern":"","position":3,"visible":true,"isBuiltin":false,"builtinType":null,"saveDir":""}]"#;
        let list: Vec<CategoryEntry> = serde_json::from_str(json).expect("parse");
        assert_eq!(list[0].extensions, vec!["epub"]);
        let back = serde_json::to_string(&list).expect("serialize");
        assert!(back.contains("\"matchMode\":\"extension\""));
    }
}
