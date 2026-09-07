//! 分类新增 / 编辑对话框：字段、校验与保存语义与
//! `lib/src/widgets/category_edit_dialog.dart` 逐条对齐。

use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    App, AppContext as _, ClickEvent, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Icon, IconName, WindowExt as _,
    button::ButtonVariant as ComponentButtonVariant,
    dialog::DialogButtonProps,
    h_flex,
    input::{Input, InputState},
    v_flex,
};

use super::categories::{CategoryEntry, read_categories, write_categories};
use crate::store::SettingsStore;

/// Dart `CategoryIcon` 名（持久化的 wire 值）→ 本地可用图标。
///
/// gpui-component 只内置一小组 Lucide 图标；没有对应矢量的名字回退到
/// 语义最接近的图标（存储值不变，Flutter 端仍按原名渲染）。
pub(crate) const CATEGORY_ICONS: &[(&str, IconName)] = &[
    ("folders", IconName::Folder),
    ("film", IconName::GalleryVerticalEnd),
    ("music", IconName::Play),
    ("fileText", IconName::File),
    ("image", IconName::GalleryVerticalEnd),
    ("archive", IconName::Inbox),
    ("file", IconName::File),
    ("code", IconName::SquareTerminal),
    ("database", IconName::HardDrive),
    ("gamepad", IconName::Bot),
    ("globe", IconName::Globe),
    ("bookmark", IconName::Star),
    ("box", IconName::Inbox),
    ("cpu", IconName::Cpu),
    ("disc", IconName::MemoryStick),
    ("font", IconName::ALargeSmall),
    ("hardDrive", IconName::HardDrive),
    ("library", IconName::BookOpen),
    ("package2", IconName::Inbox),
    ("pen", IconName::Replace),
    ("printer", IconName::Frame),
    ("smartphone", IconName::MemoryStick),
    ("subtitles", IconName::CaseSensitive),
    ("type", IconName::ALargeSmall),
    ("zap", IconName::BatteryCharging),
];

/// 分类 wire 图标名 → 渲染图标；未知名字回退通用文件图标。
pub(crate) fn icon_for(name: &str) -> IconName {
    CATEGORY_ICONS
        .iter()
        .find(|(key, _)| *key == name)
        .map_or(IconName::File, |(_, icon)| icon.clone())
}

/// 扩展名文本 → 规范化列表：逗号 / 中文逗号 / 空白分隔，去点、转小写、去空。
pub(crate) fn parse_extensions(text: &str) -> Vec<String> {
    text.split(|ch: char| ch == ',' || ch == '，' || ch.is_whitespace())
        .map(|part| part.trim().replace('.', "").to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

/// 无 `regex` 依赖的最小正则健全性检查：圆括号 / 方括号配对与尾部悬挂转义。
/// 能拦住最常见的手误；语法级校验由引擎在匹配时兜底。
pub(crate) fn regex_looks_valid(pattern: &str) -> bool {
    let mut depth = 0i32;
    let mut in_class = false;
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if chars.next().is_none() {
                    return false;
                }
            }
            '[' if !in_class => in_class = true,
            ']' if in_class => in_class = false,
            '(' if !in_class => depth += 1,
            ')' if !in_class => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !in_class
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MatchMode {
    Extension,
    Regex,
}

impl MatchMode {
    fn wire(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Regex => "regex",
        }
    }
}

pub(crate) struct CategoryDialog {
    store: Entity<SettingsStore>,
    translator: Translator,
    existing: Option<CategoryEntry>,
    name: Entity<InputState>,
    extensions: Entity<InputState>,
    regex: Entity<InputState>,
    save_dir: Entity<InputState>,
    icon: String,
    match_mode: MatchMode,
    error: Option<SharedString>,
    picking_dir: bool,
}

/// 打开新增（`existing = None`）或编辑对话框。
pub(crate) fn open(
    store: Entity<SettingsStore>,
    translator: Translator,
    existing: Option<CategoryEntry>,
    window: &mut Window,
    cx: &mut App,
) {
    let title = SharedString::from(
        translator
            .text(if existing.is_some() {
                "editCategory"
            } else {
                "addCategory"
            })
            .to_owned(),
    );
    let view = cx.new(|cx| CategoryDialog::new(store, translator, existing, window, cx));
    let name = view.read(cx).name.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let view = view.clone();
        dialog
            .title(title.clone())
            .w(px(520.))
            .content(move |content, _, _| content.child(view.clone()))
    });
    name.update(cx, |input, cx| input.focus(window, cx));
}

impl CategoryDialog {
    fn new(
        store: Entity<SettingsStore>,
        translator: Translator,
        existing: Option<CategoryEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let entry = existing.as_ref();
        let name_hint = SharedString::from(translator.text("categoryNameHint").to_owned());
        let ext_hint = SharedString::from(translator.text("extensionsHint").to_owned());
        let regex_hint = SharedString::from(translator.text("regexHint").to_owned());
        let dir_hint = SharedString::from(translator.text("selectSaveDir").to_owned());
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(entry.map(|entry| entry.name.clone()).unwrap_or_default())
                .placeholder(name_hint)
        });
        let extensions = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    entry
                        .map(|entry| entry.extensions.join(", "))
                        .unwrap_or_default(),
                )
                .placeholder(ext_hint)
        });
        let regex = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    entry
                        .map(|entry| entry.regex_pattern.clone())
                        .unwrap_or_default(),
                )
                .placeholder(regex_hint)
        });
        let save_dir = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    entry
                        .map(|entry| entry.save_dir.clone())
                        .unwrap_or_default(),
                )
                .placeholder(dir_hint)
        });
        // 目录输入变化要刷新「恢复默认」按钮的可见性。
        cx.observe(&save_dir, |_, _, cx| cx.notify()).detach();
        let match_mode = match entry.map(|entry| entry.match_mode.as_str()) {
            Some("regex") => MatchMode::Regex,
            _ => MatchMode::Extension,
        };
        Self {
            store,
            translator,
            icon: entry.map_or_else(|| "file".to_owned(), |entry| entry.icon.clone()),
            existing,
            name,
            extensions,
            regex,
            save_dir,
            match_mode,
            error: None,
            picking_dir: false,
        }
    }

    fn t(&self, key: &str) -> SharedString {
        SharedString::from(self.translator.text(key).to_owned())
    }

    fn is_builtin(&self) -> bool {
        self.existing.as_ref().is_some_and(|entry| entry.is_builtin)
    }

    fn builtin_type(&self) -> Option<&str> {
        self.existing
            .as_ref()
            .filter(|entry| entry.is_builtin)
            .and_then(|entry| entry.builtin_type.as_deref())
    }

    /// `all` 完全锁定；`other` 用排除逻辑匹配——两者都不展示匹配规则区。
    fn is_special_builtin(&self) -> bool {
        matches!(self.builtin_type(), Some("all" | "other"))
    }

    fn can_delete(&self) -> bool {
        self.existing
            .as_ref()
            .is_some_and(|entry| !entry.is_builtin)
    }

    fn fail(&mut self, key: &str, cx: &mut Context<Self>) {
        self.error = Some(self.t(key));
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() && !self.is_builtin() {
            self.fail("categoryNameRequired", cx);
            return;
        }
        let mut extensions = self
            .existing
            .as_ref()
            .map(|entry| entry.extensions.clone())
            .unwrap_or_default();
        let mut regex_pattern = self
            .existing
            .as_ref()
            .map(|entry| entry.regex_pattern.clone())
            .unwrap_or_default();
        if !self.is_special_builtin() {
            match self.match_mode {
                MatchMode::Extension => {
                    extensions = parse_extensions(&self.extensions.read(cx).value());
                    if extensions.is_empty() && !self.is_builtin() {
                        self.fail("extensionsRequired", cx);
                        return;
                    }
                    regex_pattern = String::new();
                }
                MatchMode::Regex => {
                    regex_pattern = self.regex.read(cx).value().trim().to_owned();
                    if !regex_pattern.is_empty() && !regex_looks_valid(&regex_pattern) {
                        self.fail("regexInvalid", cx);
                        return;
                    }
                    extensions = Vec::new();
                }
            }
        }
        let save_dir = self.save_dir.read(cx).value().trim().to_owned();
        let entry = match &self.existing {
            Some(existing) => CategoryEntry {
                name,
                icon: self.icon.clone(),
                match_mode: self.match_mode.wire().to_owned(),
                extensions,
                regex_pattern,
                save_dir,
                ..existing.clone()
            },
            None => CategoryEntry {
                id: format!("custom_{}", unix_ms()),
                name,
                icon: self.icon.clone(),
                match_mode: self.match_mode.wire().to_owned(),
                extensions,
                regex_pattern,
                position: 999,
                visible: true,
                is_builtin: false,
                builtin_type: None,
                save_dir,
            },
        };
        self.store.update(cx, |store, cx| {
            let mut list = read_categories(store);
            match list.iter_mut().find(|item| item.id == entry.id) {
                Some(slot) => *slot = entry,
                None => list.push(entry),
            }
            write_categories(store, list, cx);
        });
        window.close_dialog(cx);
    }

    fn confirm_delete(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.existing.as_ref().map(|entry| entry.id.clone()) else {
            return;
        };
        let store = self.store.clone();
        let title = self.t("deleteCategory");
        let description = self.t("deleteCategoryConfirm");
        let cancel = self.t("cancel");
        window.open_alert_dialog(cx, move |alert, _, _| {
            let store = store.clone();
            let id = id.clone();
            alert
                .title(title.clone())
                .description(description.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(title.clone())
                        .ok_variant(ComponentButtonVariant::Danger)
                        .cancel_text(cancel.clone())
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let id = id.clone();
                    store.update(cx, |store, cx| {
                        let list = read_categories(store)
                            .into_iter()
                            .filter(|entry| entry.id != id)
                            .collect();
                        write_categories(store, list, cx);
                    });
                    // 确认框与编辑框一起关掉；返回 false 避免基座再弹出一层。
                    window.close_all_dialogs(cx);
                    false
                })
        });
    }

    fn pick_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking_dir {
            return;
        }
        self.picking_dir = true;
        cx.notify();
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(paths))) => paths.first().map(|path| path.display().to_string()),
                _ => None,
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.picking_dir = false;
                if let Some(path) = picked {
                    this.save_dir
                        .update(cx, |input, cx| input.set_value(path, window, cx));
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn label(&self, key: &str, cx: &App) -> Div {
        let tokens = active_theme(cx).tokens();
        div()
            .text_xs()
            .font_weight(tokens.typography.sm.weight)
            .text_color(tokens.colors.muted_foreground)
            .child(self.t(key))
    }

    fn render_icon_grid(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let colors = tokens.colors;
        let mut grid = h_flex().flex_wrap().gap(tokens.spacing.xs);
        for (name, icon) in CATEGORY_ICONS {
            let selected = self.icon == *name;
            let name = *name;
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("category-icon-{name}")))
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens.radius.md)
                    .border_1()
                    .border_color(if selected {
                        colors.primary
                    } else {
                        colors.border
                    })
                    .when(selected, |this| this.bg(colors.accent))
                    .text_color(if selected {
                        colors.primary
                    } else {
                        colors.muted_foreground
                    })
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.muted))
                    .child(Icon::new(icon.clone()).size(px(14.)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.icon = name.to_owned();
                        cx.notify();
                    })),
            );
        }
        grid
    }

    fn render_mode_chip(
        &self,
        id: &'static str,
        label_key: &str,
        target: MatchMode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let variant = if self.match_mode == target {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Secondary
        };
        button(id, self.t(label_key), variant, cx).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                this.match_mode = target;
                this.error = None;
                cx.notify();
            },
        ))
    }

    fn render_match_rules(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let column = v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label("matchMode", cx))
            .child(
                h_flex()
                    .gap(tokens.spacing.sm)
                    .child(self.render_mode_chip(
                        "category-match-extension",
                        "matchByExtension",
                        MatchMode::Extension,
                        cx,
                    ))
                    .child(self.render_mode_chip(
                        "category-match-regex",
                        "matchByRegex",
                        MatchMode::Regex,
                        cx,
                    )),
            )
            .child(div().h(tokens.spacing.xs));
        match self.match_mode {
            MatchMode::Extension => column
                .child(self.label("extensionsLabel", cx))
                .child(Input::new(&self.extensions).w_full()),
            MatchMode::Regex => column
                .child(self.label("regexLabel", cx))
                .child(Input::new(&self.regex).w_full()),
        }
    }

    fn render_save_dir(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let has_value = !self.save_dir.read(cx).value().trim().is_empty();
        let mut row = h_flex()
            .gap(tokens.spacing.sm)
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.save_dir).w_full()),
            )
            .child(
                button(
                    "category-pick-dir",
                    self.t("browse"),
                    ButtonVariant::Secondary,
                    cx,
                )
                .disabled(self.picking_dir)
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.pick_dir(window, cx);
                })),
            );
        if has_value {
            row = row.child(
                button(
                    "category-clear-dir",
                    self.t("restoreDefaultPath"),
                    ButtonVariant::Ghost,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.save_dir
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    cx.notify();
                })),
            );
        }
        v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label("categorySaveDir", cx))
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t("categorySaveDirDesc")),
            )
            .child(row)
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let mut row = h_flex().w_full().items_center().gap(tokens.spacing.sm);
        if self.can_delete() {
            row = row.child(
                button(
                    "category-dialog-delete",
                    self.t("deleteCategory"),
                    ButtonVariant::Destructive,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.confirm_delete(window, cx);
                })),
            );
        }
        row.child(div().flex_1())
            .child(
                button(
                    "category-dialog-cancel",
                    self.t("cancel"),
                    ButtonVariant::Secondary,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                button(
                    "category-dialog-save",
                    self.t("confirm"),
                    ButtonVariant::Primary,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.save(window, cx))),
            )
    }
}

impl Render for CategoryDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let show_rules = !self.is_special_builtin();
        let show_dir = self.builtin_type() != Some("all");
        let mut column = v_flex()
            .w_full()
            .gap(tokens.spacing.md)
            .child(
                v_flex()
                    .gap(tokens.spacing.xs)
                    .child(self.label("categoryName", cx))
                    .child(Input::new(&self.name).w_full()),
            )
            .child(
                v_flex()
                    .gap(tokens.spacing.xs)
                    .child(self.label("categoryIcon", cx))
                    .child(self.render_icon_grid(cx)),
            );
        if show_rules {
            column = column.child(self.render_match_rules(cx));
        }
        if show_dir {
            column = column.child(self.render_save_dir(cx));
        }
        if let Some(error) = self.error.clone() {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(tokens.colors.destructive)
                    .child(error),
            );
        }
        column.child(self.render_footer(cx))
    }
}

/// 当前 Unix 毫秒；时钟异常时退化为 0（仅用于生成 id）。
pub(crate) fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extensions_like_dart() {
        assert_eq!(
            parse_extensions(" .EPUB, mobi，azw3  txt "),
            vec!["epub", "mobi", "azw3", "txt"]
        );
        assert!(parse_extensions(" , ").is_empty());
    }

    #[test]
    fn regex_sanity_check() {
        assert!(regex_looks_valid(r".*\.(epub|mobi)$"));
        assert!(regex_looks_valid(r"[a-z)]+\{2}"));
        assert!(!regex_looks_valid(r"(abc"));
        assert!(!regex_looks_valid(r"abc)"));
        assert!(!regex_looks_valid(r"[abc"));
        assert!(!regex_looks_valid(r"abc\"));
    }

    #[test]
    fn icon_lookup_falls_back_to_file() {
        assert!(matches!(icon_for("cpu"), IconName::Cpu));
        assert!(matches!(icon_for("nope"), IconName::File));
    }
}
