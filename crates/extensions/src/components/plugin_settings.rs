//! 单个插件的设置表单：按 widget 类型分发输入控件，提交前做
//! required / number / min-max / select 前置校验，全部通过才发起
//! `daemon.plugin.updateSettings`（`pattern` 由 daemon 侧 JS RegExp 校验）。

use std::{collections::HashMap, sync::Arc};

use fluxdown_protocol::{PluginDto, RpcErrorData, SettingFieldDto};
use fluxdown_ui_i18n::Translator;
use gpui::{
    Anchor, AppContext as _, ClipboardItem, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    button::Button,
    h_flex,
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    switch::Switch,
    v_flex,
};

use crate::{ExtensionsPort, controller::update_plugin_settings, error_text};

/// 前置校验失败原因；文案映射留给 UI 层。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldError {
    Required,
    Number,
    Min(String),
    Max(String),
    Select,
    /// daemon 校验失败并指明了字段（如 `pattern` 不匹配）。
    Server(String),
}

/// 单条设置项前置校验：required → number/min/max → select 成员。
pub fn validate_field(field: &SettingFieldDto, raw: &str) -> Option<FieldError> {
    let value = raw.trim();
    if field.required && value.is_empty() {
        return Some(FieldError::Required);
    }
    if value.is_empty() {
        return None;
    }
    if field.setting_type == "number" {
        let Ok(number) = value.parse::<f64>() else {
            return Some(FieldError::Number);
        };
        if let Some(min) = field.min
            && number < min
        {
            return Some(FieldError::Min(trim_number(min)));
        }
        if let Some(max) = field.max
            && number > max
        {
            return Some(FieldError::Max(trim_number(max)));
        }
    }
    if field.widget == "select"
        && !field.options.is_empty()
        && !field.options.iter().any(|option| option.value == value)
    {
        return Some(FieldError::Select);
    }
    None
}

/// 去掉整数值的多余小数位（3.0 → "3"，3.5 → "3.5"）。
pub fn trim_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

/// 字段初始值：已保存值优先，否则 manifest 默认值（toggle 默认 `false`）。
fn initial_value(field: &SettingFieldDto, saved: &HashMap<String, String>) -> String {
    if let Some(value) = saved.get(&field.key) {
        return value.clone();
    }
    match field.default.as_deref() {
        Some(default) if !default.is_empty() => default.to_owned(),
        _ if field.widget == "toggle" => "false".to_owned(),
        _ => String::new(),
    }
}

fn range_hint(field: &SettingFieldDto) -> Option<String> {
    match (field.min, field.max) {
        (Some(min), Some(max)) => Some(format!("{} – {}", trim_number(min), trim_number(max))),
        (Some(min), None) => Some(format!("≥ {}", trim_number(min))),
        (None, Some(max)) => Some(format!("≤ {}", trim_number(max))),
        (None, None) => None,
    }
}

enum FieldControl {
    Text(Entity<InputState>),
    Textarea(Entity<TextareaState>),
    Toggle(bool),
    Select(String),
    Folder(String),
}

pub struct PluginSettingsForm {
    translator: Entity<Translator>,
    port: Arc<dyn ExtensionsPort>,
    identity: String,
    fields: Vec<SettingFieldDto>,
    controls: Vec<FieldControl>,
    errors: HashMap<String, FieldError>,
    server_error: Option<SharedString>,
    saving: bool,
}

impl PluginSettingsForm {
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn ExtensionsPort>,
        plugin: &PluginDto,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let controls = plugin
            .settings
            .iter()
            .map(|field| {
                let value = initial_value(field, &plugin.settings_values);
                match field.widget.as_str() {
                    "toggle" => FieldControl::Toggle(value == "true"),
                    "select" => FieldControl::Select(value),
                    "folder" => FieldControl::Folder(value),
                    "textarea" => FieldControl::Textarea(
                        cx.new(|cx| TextareaState::new(window, cx).default_value(value)),
                    ),
                    widget => {
                        let placeholder = if field.setting_type == "number" {
                            range_hint(field).unwrap_or_default()
                        } else {
                            field.default.clone().unwrap_or_default()
                        };
                        FieldControl::Text(cx.new(|cx| {
                            InputState::new(window, cx)
                                .masked(widget == "password")
                                .default_value(value)
                                .placeholder(placeholder)
                        }))
                    }
                }
            })
            .collect();
        Self {
            translator,
            port,
            identity: plugin.identity.clone(),
            fields: plugin.settings.clone(),
            controls,
            errors: HashMap::new(),
            server_error: None,
            saving: false,
        }
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    fn value_of(&self, index: usize, cx: &Context<Self>) -> String {
        match &self.controls[index] {
            FieldControl::Text(input) => input.read(cx).value().to_string(),
            FieldControl::Textarea(input) => input.read(cx).value().to_string(),
            FieldControl::Toggle(checked) => if *checked { "true" } else { "false" }.to_owned(),
            FieldControl::Select(value) | FieldControl::Folder(value) => value.clone(),
        }
    }

    /// 校验并提交；校验失败只标红字段，成功后关闭所在对话框。
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let mut entries = HashMap::with_capacity(self.fields.len());
        let mut errors = HashMap::new();
        for (index, field) in self.fields.iter().enumerate() {
            let value = self.value_of(index, cx);
            if let Some(error) = validate_field(field, &value) {
                errors.insert(field.key.clone(), error);
            }
            entries.insert(field.key.clone(), value);
        }
        self.server_error = None;
        self.errors = errors;
        if !self.errors.is_empty() {
            cx.notify();
            return;
        }
        self.saving = true;
        cx.notify();
        let future = update_plugin_settings(&self.port, &self.identity, entries);
        cx.spawn_in(window, async move |this, cx| {
            let result = future.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(_) => window.close_dialog(cx),
                    Err(error) => this.apply_server_error(&error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_server_error(&mut self, error: &RpcErrorData, cx: &Context<Self>) {
        let translator = self.translator.read(cx);
        let message = error_text(translator, error);
        self.server_error = Some(SharedString::from(
            translator.text_with("pluginSettingsSaveFailed", &[("message", &message)]),
        ));
        if let Some(field) = error
            .field
            .as_deref()
            .filter(|field| self.fields.iter().any(|known| known.key == *field))
        {
            self.errors
                .insert(field.to_owned(), FieldError::Server(message));
        }
    }

    fn error_text(&self, key: &str, cx: &Context<Self>) -> Option<SharedString> {
        let translator = self.translator.read(cx);
        self.errors.get(key).map(|error| match error {
            FieldError::Required => {
                SharedString::from(translator.text("pluginErrRequired").to_owned())
            }
            FieldError::Number => SharedString::from(translator.text("pluginErrNumber").to_owned()),
            FieldError::Min(min) => {
                SharedString::from(translator.text_with("pluginErrMin", &[("min", min)]))
            }
            FieldError::Max(max) => {
                SharedString::from(translator.text_with("pluginErrMax", &[("max", max)]))
            }
            FieldError::Select => SharedString::from(translator.text("pluginErrSelect").to_owned()),
            FieldError::Server(message) => SharedString::from(message.clone()),
        })
    }

    fn render_control(
        &self,
        index: usize,
        field: &SettingFieldDto,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let disabled = self.saving;
        match &self.controls[index] {
            FieldControl::Text(input) => Input::new(input)
                .w_full()
                .disabled(disabled)
                .when(field.widget == "password", Input::mask_toggle)
                .into_any_element(),
            FieldControl::Textarea(input) => Textarea::new(input)
                .w_full()
                .h(px(96.))
                .disabled(disabled)
                .into_any_element(),
            FieldControl::Toggle(checked) => Switch::new(("plugin-setting-toggle", index))
                .checked(*checked)
                .disabled(disabled)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    this.controls[index] = FieldControl::Toggle(*checked);
                    this.clear_error(index);
                    cx.notify();
                }))
                .into_any_element(),
            FieldControl::Select(value) => {
                let label = field
                    .options
                    .iter()
                    .find(|option| option.value == *value)
                    .map(|option| option.label.clone())
                    .unwrap_or_else(|| {
                        if value.is_empty() {
                            self.translator
                                .read(cx)
                                .text("pluginSelectPlaceholder")
                                .to_owned()
                        } else {
                            value.clone()
                        }
                    });
                let options = field
                    .options
                    .iter()
                    .map(|option| (option.value.clone(), option.label.clone()))
                    .collect::<Vec<_>>();
                let current = value.clone();
                let form = cx.entity();
                Button::new(("plugin-setting-select", index))
                    .label(label)
                    .outline()
                    .small()
                    .dropdown_caret(true)
                    .disabled(disabled)
                    .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
                        options
                            .iter()
                            .fold(menu, |menu, (option_value, option_label)| {
                                let checked = *option_value == current;
                                let form = form.clone();
                                let option_value = option_value.clone();
                                menu.item(
                                    PopupMenuItem::new(option_label.clone())
                                        .checked(checked)
                                        .on_click(move |_, _, cx| {
                                            form.update(cx, |form, cx| {
                                                form.controls[index] =
                                                    FieldControl::Select(option_value.clone());
                                                form.clear_error(index);
                                                cx.notify();
                                            });
                                        }),
                                )
                            })
                    })
                    .into_any_element()
            }
            FieldControl::Folder(path) => {
                let translator = self.translator.read(cx);
                let placeholder = translator.text("pluginFolderPickPlaceholder").to_owned();
                let muted = cx.theme().muted_foreground;
                let foreground = cx.theme().foreground;
                let display = if path.is_empty() {
                    placeholder.clone()
                } else {
                    path.clone()
                };
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(if path.is_empty() { muted } else { foreground })
                            .child(display),
                    )
                    .child(
                        Button::new(("plugin-setting-folder", index))
                            .outline()
                            .small()
                            .icon(IconName::FolderOpen)
                            .label(placeholder)
                            .disabled(disabled)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.pick_folder(index, window, cx);
                            })),
                    )
                    .into_any_element()
            }
        }
    }

    fn clear_error(&mut self, index: usize) {
        if let Some(field) = self.fields.get(index) {
            self.errors.remove(&field.key);
        }
    }

    fn pick_folder(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.first()
            {
                let text = path.display().to_string();
                let _ = this.update(cx, |this, cx| {
                    this.controls[index] = FieldControl::Folder(text);
                    this.clear_error(index);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn copy_helper_script(&self, script: String, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(script));
        let message = self
            .translator
            .read(cx)
            .text("pluginHelperScriptCopied")
            .to_owned();
        window.push_notification(Notification::success(message), cx);
    }
}

impl Render for PluginSettingsForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let copy_label = self
            .translator
            .read(cx)
            .text("pluginCopyHelperScript")
            .to_owned();
        let rows = (0..self.fields.len())
            .map(|index| {
                let field = &self.fields[index];
                let title = if field.title.is_empty() {
                    field.key.clone()
                } else {
                    field.title.clone()
                };
                let error = self.error_text(&field.key, cx);
                let helper = field
                    .helper_script
                    .clone()
                    .filter(|script| !script.is_empty())
                    .map(|script| {
                        let label = field
                            .helper_label
                            .clone()
                            .filter(|label| !label.is_empty())
                            .unwrap_or_else(|| copy_label.clone());
                        Button::new(("plugin-setting-helper", index))
                            .outline()
                            .small()
                            .icon(gpui_component::IconName::Copy)
                            .label(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.copy_helper_script(script.clone(), window, cx);
                            }))
                    });
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(div().text_sm().font_semibold().child(title))
                    .when(!field.description.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(field.description.clone()),
                        )
                    })
                    .child(self.render_control(index, field, cx))
                    .children(helper)
                    .when_some(error, |this, error| {
                        this.child(div().text_xs().text_color(theme.danger).child(error))
                    })
            })
            .collect::<Vec<_>>();
        v_flex()
            .id("plugin-settings-form")
            .w_full()
            .max_h(px(420.))
            .overflow_y_scroll()
            .gap_4()
            .px_1()
            .py_1()
            .when_some(self.server_error.clone(), |this, error| {
                this.child(
                    div()
                        .w_full()
                        .p_2()
                        .rounded(theme.radius)
                        .bg(theme.danger.opacity(0.12))
                        .text_xs()
                        .text_color(theme.danger)
                        .child(error),
                )
            })
            .children(rows)
    }
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{SettingFieldDto, SettingOptionDto};

    use super::{FieldError, trim_number, validate_field};

    fn field(setting_type: &str, widget: &str) -> SettingFieldDto {
        SettingFieldDto {
            key: "k".to_owned(),
            title: "K".to_owned(),
            description: String::new(),
            setting_type: setting_type.to_owned(),
            widget: widget.to_owned(),
            options: Vec::new(),
            default: None,
            required: false,
            min: None,
            max: None,
            pattern: None,
            helper_script: None,
            helper_label: None,
        }
    }

    #[test]
    fn required_rejects_blank_only() {
        let mut required = field("string", "text");
        required.required = true;
        assert_eq!(validate_field(&required, "  "), Some(FieldError::Required));
        assert_eq!(validate_field(&required, "x"), None);
        assert_eq!(validate_field(&field("string", "text"), ""), None);
    }

    #[test]
    fn number_range_is_enforced() {
        let mut number = field("number", "number");
        number.min = Some(1.0);
        number.max = Some(10.5);
        assert_eq!(validate_field(&number, "abc"), Some(FieldError::Number));
        assert_eq!(
            validate_field(&number, "0"),
            Some(FieldError::Min("1".to_owned()))
        );
        assert_eq!(
            validate_field(&number, "11"),
            Some(FieldError::Max("10.5".to_owned()))
        );
        assert_eq!(validate_field(&number, " 5 "), None);
    }

    #[test]
    fn select_requires_known_option() {
        let mut select = field("string", "select");
        select.options.push(SettingOptionDto {
            value: "a".to_owned(),
            label: "A".to_owned(),
        });
        assert_eq!(validate_field(&select, "b"), Some(FieldError::Select));
        assert_eq!(validate_field(&select, "a"), None);
    }

    #[test]
    fn trim_number_drops_integer_fraction() {
        assert_eq!(trim_number(3.0), "3");
        assert_eq!(trim_number(3.5), "3.5");
        assert_eq!(trim_number(-2.0), "-2");
    }
}
