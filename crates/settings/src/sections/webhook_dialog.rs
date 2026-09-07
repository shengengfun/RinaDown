//! Webhook 端点新增 / 编辑对话框：字段、校验、实时预览与测试投递语义与
//! `lib/src/widgets/webhook_endpoint_dialog.dart` 逐条对齐。

use std::collections::{BTreeMap, BTreeSet};

use fluxdown_protocol::{RpcErrorData, WebhookDeliveriesResponse, WebhookPresetDto, method};
use fluxdown_ui_components::{ButtonVariant, button};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    Anchor, App, AppContext as _, ClickEvent, ClipboardItem, Context, Div, Entity,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use gpui_component::{
    Icon, IconName, WindowExt as _,
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    switch::Switch,
    v_flex,
};
use serde_json::{Value, json};

use super::webhook::{EndpointSpec, WEBHOOK_EVENTS, read_endpoints, write_endpoints};
use crate::store::SettingsStore;

const PRESET_CUSTOM: &str = "custom";
/// 与 Dart `WebhookEvents.defaults` 一致。
const DEFAULT_EVENTS: &[&str] = &["task.completed", "task.failed"];

/// 预览用样例变量——与引擎 `WebhookEvent::sample()` 对齐，仅用于预览。
const SAMPLE_VARS: &[(&str, &str)] = &[
    ("{event}", "task.completed"),
    ("{event.title}", "Download completed"),
    (
        "{event.summary}",
        "ubuntu-24.04.2-desktop-amd64.iso · 6.0 GB",
    ),
    ("{timestamp}", "2026-07-17T12:34:56Z"),
    ("{instance.app}", "fluxdown"),
    ("{instance.version}", "0.1.44"),
    ("{instance.host}", "DESKTOP"),
    ("{task.id}", "00000000-0000-4000-8000-000000000000"),
    ("{task.fileName}", "ubuntu-24.04.2-desktop-amd64.iso"),
    ("{task.url}", "https://releases.ubuntu.com/24.04/ubuntu.iso"),
    ("{task.saveDir}", "/downloads"),
    ("{task.totalBytes}", "6442450944"),
    ("{task.totalBytesHuman}", "6.0 GB"),
    ("{task.status}", "3"),
    ("{task.errorMessage}", ""),
    ("{queue.id}", "main"),
    ("{queue.name}", "Main"),
    ("{ntfy.topic}", "my-topic"),
];

fn sample_var(key: &str) -> Option<&'static str> {
    SAMPLE_VARS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

/// `application/x-www-form-urlencoded` 组件编码（Dart `Uri.encodeQueryComponent`）。
pub(crate) fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(byte as char),
            b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

/// JSON 字符串上下文转义：借 serde 转义后剥掉外层引号。
fn json_escape(value: &str) -> String {
    let encoded = serde_json::to_string(value).unwrap_or_default();
    encoded
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .map_or(encoded.clone(), str::to_owned)
}

/// 占位符替换——与引擎 `render_template` 同规则：占位符是不含嵌套 `{` 的
/// `{…}` 段，未知段原样保留，因此 JSON 字面量不会被破坏。
pub(crate) fn render_preview(template: &str, form_escape: bool) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < template.len() {
        if bytes[i] != b'{' {
            let start = i;
            while i < template.len() && bytes[i] != b'{' {
                i += 1;
            }
            out.push_str(&template[start..i]);
            continue;
        }
        let mut j = i + 1;
        while j < template.len() && bytes[j] != b'}' && bytes[j] != b'{' {
            j += 1;
        }
        if j >= template.len() || bytes[j] == b'{' {
            out.push('{');
            i += 1;
            continue;
        }
        let key = &template[i..=j];
        match sample_var(key) {
            None => out.push_str(key),
            Some(value) if form_escape => out.push_str(&form_encode(value)),
            Some(value) => out.push_str(&json_escape(value)),
        }
        i = j + 1;
    }
    out
}

/// 无模板（custom 预设）时的 §3.2 信封原文。
fn envelope_preview() -> String {
    let value = json!({
        "schemaVersion": 1,
        "event": sample_var("{event}"),
        "deliveryId": "5f2a91c7-8b3e-4d10-a6f4-c2d90b7e13aa",
        "timestamp": sample_var("{timestamp}"),
        "instance": {
            "app": sample_var("{instance.app}"),
            "version": sample_var("{instance.version}"),
            "host": sample_var("{instance.host}"),
        },
        "queue": {
            "id": sample_var("{queue.id}"),
            "name": sample_var("{queue.name}"),
        },
        "task": {
            "id": sample_var("{task.id}"),
            "fileName": sample_var("{task.fileName}"),
            "url": sample_var("{task.url}"),
            "saveDir": sample_var("{task.saveDir}"),
            "totalBytes": 6_442_450_944_i64,
            "status": 3,
            "errorMessage": "",
        },
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// URL 内联校验错误的文案键；`None` = 通过。空 URL 由保存按钮禁用兜底。
pub(crate) fn url_error_key(raw: &str, allow_http: bool) -> Option<&'static str> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let Some((scheme, rest)) = raw.split_once("://") else {
        return Some("webhookUrlInvalid");
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return Some("webhookUrlInvalid");
    }
    match scheme.to_ascii_lowercase().as_str() {
        "https" => None,
        "http" if allow_http => None,
        "http" => Some("webhookUrlWarnHttp"),
        _ => Some("webhookUrlInvalid"),
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    #[cfg(unix)]
    {
        use std::io::Read as _;
        if std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut bytes))
            .is_ok()
        {
            return bytes;
        }
    }
    // 回退：以 OS 随机种子的 SipHash 混合时间与计数器。
    use std::hash::{BuildHasher as _, Hasher as _};
    let state = std::collections::hash_map::RandomState::new();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    for (index, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut hasher = state.build_hasher();
        hasher.write_u128(nanos);
        hasher.write_usize(index);
        hasher.write_u32(std::process::id());
        let word = hasher.finish().to_le_bytes();
        chunk.copy_from_slice(&word[..chunk.len()]);
    }
    bytes
}

/// HMAC 密钥起点（`whsec_` + 32 位十六进制），与 Dart `generateWebhookSecret` 同形。
pub(crate) fn generate_secret() -> String {
    let mut out = String::with_capacity(6 + 32);
    out.push_str("whsec_");
    for byte in random_bytes::<16>() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

struct HeaderRow {
    id: usize,
    key: Entity<InputState>,
    value: Entity<InputState>,
}

struct TestOutcome {
    success: bool,
    text: SharedString,
}

pub(crate) struct WebhookDialog {
    store: Entity<SettingsStore>,
    translator: Translator,
    existing: Option<EndpointSpec>,
    name: Entity<InputState>,
    url: Entity<InputState>,
    template: Entity<TextareaState>,
    secret: Entity<InputState>,
    preset: String,
    presets: Vec<WebhookPresetDto>,
    variables: Vec<String>,
    events: BTreeSet<String>,
    queue_id: String,
    headers: Vec<HeaderRow>,
    header_seq: usize,
    sign_enabled: bool,
    allow_http: bool,
    use_proxy: bool,
    advanced_open: bool,
    url_touched: bool,
    testing: bool,
    test_result: Option<TestOutcome>,
    copied: bool,
}

/// 打开新增（`existing = None`）或编辑对话框。
pub(crate) fn open(
    store: Entity<SettingsStore>,
    translator: Translator,
    existing: Option<EndpointSpec>,
    window: &mut Window,
    cx: &mut App,
) {
    let title = SharedString::from(
        translator
            .text(if existing.is_some() {
                "webhookDialogEditTitle"
            } else {
                "webhookDialogAddTitle"
            })
            .to_owned(),
    );
    let view = cx.new(|cx| WebhookDialog::new(store, translator, existing, window, cx));
    let name = view.read(cx).name.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let view = view.clone();
        dialog
            .title(title.clone())
            .w(px(880.))
            .margin_top(px(32.))
            .content(move |content, _, _| content.child(view.clone()))
    });
    name.update(cx, |input, cx| input.focus(window, cx));
}

impl WebhookDialog {
    fn new(
        store: Entity<SettingsStore>,
        translator: Translator,
        existing: Option<EndpointSpec>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let entry = existing.as_ref();
        let template_hint =
            SharedString::from(translator.text("webhookTemplatePlaceholder").to_owned());
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(entry.map(|entry| entry.name.clone()).unwrap_or_default())
        });
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(entry.map(|entry| entry.url.clone()).unwrap_or_default())
        });
        let template = cx.new(|cx| {
            TextareaState::new(window, cx)
                .default_value(
                    entry
                        .map(|entry| entry.body_template.clone())
                        .unwrap_or_default(),
                )
                .placeholder(template_hint)
        });
        let secret = cx.new(|cx| {
            InputState::new(window, cx).default_value(
                entry
                    .map(|entry| entry.sign_secret.clone())
                    .unwrap_or_default(),
            )
        });
        // 名称 / URL / 模板 / 密钥变化都要刷新预览与保存按钮状态。
        cx.observe(&name, |_, _, cx| cx.notify()).detach();
        cx.observe(&url, |_, _, cx| cx.notify()).detach();
        cx.observe(&template, |_, _, cx| cx.notify()).detach();
        cx.observe(&secret, |_, _, cx| cx.notify()).detach();
        // 失焦才亮红字：边打字边报错是噪音，不是帮助。
        cx.subscribe(&url, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                this.url_touched = true;
                cx.notify();
            }
        })
        .detach();

        let mut header_seq = 0;
        let headers = entry
            .map(|entry| entry.headers.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|(key, value)| {
                Self::header_row(&mut header_seq, &translator, &key, &value, window, cx)
            })
            .collect::<Vec<_>>();
        let sign_enabled = entry.is_some_and(|entry| !entry.sign_secret.is_empty());
        let allow_http = entry.is_some_and(|entry| entry.allow_http);
        let use_proxy = entry.is_some_and(|entry| entry.use_proxy);
        let has_template = entry.is_some_and(|entry| !entry.body_template.is_empty());
        let events = entry.map_or_else(
            || {
                DEFAULT_EVENTS
                    .iter()
                    .map(|event| (*event).to_owned())
                    .collect()
            },
            |entry| entry.events.iter().cloned().collect(),
        );

        let mut this = Self {
            store,
            translator,
            preset: entry.map_or_else(|| PRESET_CUSTOM.to_owned(), |entry| entry.preset.clone()),
            presets: Vec::new(),
            variables: Vec::new(),
            events,
            queue_id: entry
                .map(|entry| entry.queue_id.clone())
                .unwrap_or_default(),
            // 已有自定义头 / 模板 / 签名的端点直接展开高级区。
            advanced_open: !headers.is_empty()
                || has_template
                || sign_enabled
                || allow_http
                || use_proxy,
            headers,
            header_seq,
            sign_enabled,
            allow_http,
            use_proxy,
            url_touched: false,
            testing: false,
            test_result: None,
            copied: false,
            existing,
            name,
            url,
            template,
            secret,
        };
        this.load_presets(cx);
        this
    }

    fn header_row(
        seq: &mut usize,
        translator: &Translator,
        key: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> HeaderRow {
        *seq += 1;
        let key_hint = SharedString::from(translator.text("webhookHeaderName").to_owned());
        let value_hint = SharedString::from(translator.text("webhookHeaderValue").to_owned());
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(key.to_owned())
                .placeholder(key_hint)
        });
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value.to_owned())
                .placeholder(value_hint)
        });
        HeaderRow {
            id: *seq,
            key,
            value,
        }
    }

    /// 预设目录 + 变量清单来自引擎（`daemon.webhook.get`），本地不复制模板内容。
    fn load_presets(&mut self, cx: &mut Context<Self>) {
        let future = self
            .store
            .read(cx)
            .raw_call(method::DAEMON_WEBHOOK_GET, json!({}));
        cx.spawn(async move |this, cx| {
            let Ok(value) = future.await else {
                return;
            };
            let Ok(response) = serde_json::from_value::<WebhookDeliveriesResponse>(value) else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.presets = response.presets;
                this.variables = response.variables;
                cx.notify();
            });
        })
        .detach();
    }

    fn t(&self, key: &str) -> SharedString {
        SharedString::from(self.translator.text(key).to_owned())
    }

    fn current_preset(&self) -> Option<&WebhookPresetDto> {
        self.presets.iter().find(|preset| preset.id == self.preset)
    }

    fn url_error(&self, cx: &App) -> Option<&'static str> {
        url_error_key(&self.url.read(cx).value(), self.allow_http)
    }

    fn can_save(&self, cx: &App) -> bool {
        !self.name.read(cx).value().trim().is_empty()
            && !self.url.read(cx).value().trim().is_empty()
            && self.url_error(cx).is_none()
    }

    /// 草稿 → 模型（与 Dart `_draft` 同序同规则）。
    fn draft(&self, cx: &App) -> EndpointSpec {
        let mut headers = BTreeMap::new();
        for row in &self.headers {
            let key = row.key.read(cx).value().trim().to_owned();
            if key.is_empty() {
                continue;
            }
            headers.insert(key, row.value.read(cx).value().to_string());
        }
        let existing = self.existing.as_ref();
        EndpointSpec {
            id: existing.map_or_else(
                || format!("wh_{}", super::category_dialog::unix_ms()),
                |entry| entry.id.clone(),
            ),
            name: self.name.read(cx).value().trim().to_owned(),
            preset: self.preset.clone(),
            url: self.url.read(cx).value().trim().to_owned(),
            enabled: existing.is_none_or(|entry| entry.enabled),
            events: WEBHOOK_EVENTS
                .iter()
                .map(|(wire, _)| (*wire).to_owned())
                .filter(|wire| self.events.contains(wire))
                .collect(),
            queue_id: self.queue_id.clone(),
            headers,
            body_template: self.template.read(cx).value().to_string(),
            sign_secret: if self.sign_enabled {
                self.secret.read(cx).value().trim().to_owned()
            } else {
                String::new()
            },
            allow_http: self.allow_http,
            use_proxy: self.use_proxy,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_save(cx) {
            return;
        }
        let draft = self.draft(cx);
        self.store.update(cx, |store, cx| {
            let mut list = read_endpoints(store);
            match list.iter_mut().find(|entry| entry.id == draft.id) {
                Some(slot) => *slot = draft,
                None => list.push(draft),
            }
            write_endpoints(store, &list, cx);
        });
        window.close_dialog(cx);
    }

    /// 页脚「发送测试」：把当前草稿直接交给引擎，无需先保存。
    fn send_test(&mut self, cx: &mut Context<Self>) {
        if self.testing {
            return;
        }
        let params = serde_json::to_value(self.draft(cx)).unwrap_or_else(|_| json!({}));
        let future = self
            .store
            .read(cx)
            .raw_call(method::DAEMON_WEBHOOK_TEST, params);
        self.testing = true;
        self.test_result = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = future.await;
            let _ = this.update(cx, |this, cx| {
                this.testing = false;
                this.test_result = Some(this.test_outcome(result));
                cx.notify();
            });
        })
        .detach();
    }

    fn test_outcome(&self, result: Result<Value, RpcErrorData>) -> TestOutcome {
        match result {
            Ok(value) => {
                let success = value
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let status = value.get("statusCode").and_then(Value::as_i64).unwrap_or(0);
                let latency = value
                    .get("latencyMs")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .to_string();
                let error = value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let text = if success {
                    let status = if status == 0 {
                        "OK".to_owned()
                    } else {
                        status.to_string()
                    };
                    self.translator
                        .text_with("webhookTestOk", &[("status", &status), ("ms", &latency)])
                } else {
                    let detail = if error.is_empty() {
                        status.to_string()
                    } else {
                        error
                    };
                    self.translator
                        .text_with("webhookTestFail", &[("error", &detail)])
                };
                TestOutcome {
                    success,
                    text: SharedString::from(text),
                }
            }
            Err(error) => TestOutcome {
                success: false,
                text: SharedString::from(self.translator.text_with(
                    "webhookTestFail",
                    &[("error", &format!("{:?}", error.code))],
                )),
            },
        }
    }

    fn insert_variable(&mut self, variable: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.template.update(cx, |template, cx| {
            template.insert(variable.to_owned(), window, cx);
            template.focus(window, cx);
        });
        cx.notify();
    }

    /// 复制密钥；按钮文案短暂变为「已复制」（对应 Flutter 的 2s toast）。
    fn copy_secret(&mut self, cx: &mut Context<Self>) {
        let text = self.secret.read(cx).value().to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        })
        .detach();
    }

    // ───────────────────────── 渲染 ─────────────────────────

    fn label(&self, key: &str, cx: &App) -> Div {
        let tokens = active_theme(cx).tokens();
        div()
            .text_xs()
            .font_weight(tokens.typography.sm.weight)
            .text_color(tokens.colors.muted_foreground)
            .child(self.t(key))
    }

    fn hint(&self, text: SharedString, cx: &App) -> Div {
        let tokens = active_theme(cx).tokens();
        div()
            .text_xs()
            .text_color(tokens.colors.muted_foreground)
            .child(text)
    }

    fn render_preset_grid(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let mut grid = h_flex().flex_wrap().gap(tokens.spacing.sm);
        for preset in &self.presets {
            let selected = preset.id == self.preset;
            let id = preset.id.clone();
            grid = grid.child(
                button(
                    SharedString::from(format!("webhook-preset-{}", preset.id)),
                    SharedString::from(preset.label.clone()),
                    if selected {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    },
                    cx,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.preset = id.clone();
                    cx.notify();
                })),
            );
        }
        grid
    }

    fn render_url_field(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let error = if self.url_touched {
            self.url_error(cx)
        } else {
            None
        };
        let hint_key = match error {
            Some(key) => key,
            None if self.preset == "ntfy" => "webhookUrlHintNtfy",
            None => "webhookUrlHint",
        };
        v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label("webhookFieldUrl", cx))
            .child(Input::new(&self.url).w_full())
            .child(
                div()
                    .text_xs()
                    .text_color(if error.is_some() {
                        tokens.colors.destructive
                    } else {
                        tokens.colors.muted_foreground
                    })
                    .child(self.t(hint_key)),
            )
    }

    fn render_events(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let empty = self.events.is_empty();
        let mut chips = h_flex().flex_wrap().gap(tokens.spacing.sm);
        for (wire, label_key) in WEBHOOK_EVENTS {
            let checked = self.events.contains(*wire);
            let wire = *wire;
            chips = chips.child(
                Checkbox::new(SharedString::from(format!("webhook-event-{wire}")))
                    .label(self.t(label_key))
                    .checked(checked)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        if *checked {
                            this.events.insert(wire.to_owned());
                        } else {
                            this.events.remove(wire);
                        }
                        cx.notify();
                    })),
            );
        }
        v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label("webhookFieldEvents", cx))
            .child(
                div()
                    .text_xs()
                    .text_color(if empty {
                        tokens.colors.destructive
                    } else {
                        tokens.colors.muted_foreground
                    })
                    .child(self.t(if empty {
                        "webhookEventsEmpty"
                    } else {
                        "webhookEventsHint"
                    })),
            )
            .child(chips)
    }

    fn queue_name(&self, id: &str, cx: &App) -> SharedString {
        if id.is_empty() {
            return self.t("webhookQueueAll");
        }
        self.store
            .read(cx)
            .queues()
            .iter()
            .find(|queue| queue.queue_id == id)
            .map_or_else(
                || SharedString::from(id.to_owned()),
                |queue| SharedString::from(queue.name.clone()),
            )
    }

    fn render_queue_filter(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let mut options = vec![(String::new(), self.t("webhookQueueAll"))];
        options.extend(self.store.read(cx).queues().iter().map(|queue| {
            (
                queue.queue_id.clone(),
                SharedString::from(queue.name.clone()),
            )
        }));
        let current = self.queue_id.clone();
        let this = cx.weak_entity();
        v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label("webhookFieldQueue", cx))
            .child(
                gpui_component::button::Button::new("webhook-queue")
                    .label(self.queue_name(&self.queue_id, cx))
                    .dropdown_caret(true)
                    .outline()
                    .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
                        options.iter().fold(menu, |menu, (value, label)| {
                            let this = this.clone();
                            let value = value.clone();
                            menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(*value == current)
                                    .on_click(move |_, _, cx| {
                                        let value = value.clone();
                                        let _ = this.update(cx, |this, cx| {
                                            this.queue_id = value;
                                            cx.notify();
                                        });
                                    }),
                            )
                        })
                    }),
            )
    }

    fn render_headers(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let mut column = v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label("webhookFieldHeaders", cx));
        for row in &self.headers {
            let row_id = row.id;
            column = column.child(
                h_flex()
                    .gap(tokens.spacing.sm)
                    .items_center()
                    .child(div().w(px(150.)).child(Input::new(&row.key).w_full()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&row.value).w_full()),
                    )
                    .child(
                        button(
                            SharedString::from(format!("webhook-header-remove-{row_id}")),
                            self.t("webhookRowDelete"),
                            ButtonVariant::Ghost,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.headers.retain(|row| row.id != row_id);
                                cx.notify();
                            },
                        )),
                    ),
            );
        }
        column.child(
            div().child(
                button(
                    "webhook-header-add",
                    self.t("webhookAddHeader"),
                    ButtonVariant::Secondary,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    let mut seq = this.header_seq;
                    let translator = this.translator.clone();
                    let row = Self::header_row(&mut seq, &translator, "", "", window, cx);
                    this.header_seq = seq;
                    this.headers.push(row);
                    cx.notify();
                })),
            ),
        )
    }

    fn render_template(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let mut chips = h_flex().flex_wrap().gap(tokens.spacing.xs);
        for variable in &self.variables {
            let insert = variable.clone();
            chips = chips.child(
                button(
                    SharedString::from(format!("webhook-var-{variable}")),
                    SharedString::from(variable.clone()),
                    ButtonVariant::Ghost,
                    cx,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.insert_variable(&insert, window, cx);
                })),
            );
        }
        v_flex()
            .gap(tokens.spacing.xs)
            .child(self.label("webhookFieldTemplate", cx))
            .child(
                Textarea::new(&self.template)
                    .h(px(96.))
                    .w_full()
                    .font_family(tokens.typography.mono.clone()),
            )
            .child(self.hint(self.t("webhookTemplateHint"), cx))
            .child(chips)
    }

    fn render_switch_row(
        &self,
        id: &'static str,
        title_key: &str,
        desc_key: &str,
        checked: bool,
        on_change: impl Fn(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        h_flex()
            .gap(tokens.spacing.lg)
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(tokens.spacing.xxs)
                    .child(div().text_sm().child(self.t(title_key)))
                    .child(self.hint(self.t(desc_key), cx)),
            )
            .child(Switch::new(id).checked(checked).on_click(cx.listener(
                move |this, checked: &bool, window, cx| {
                    on_change(this, *checked, window, cx);
                    cx.notify();
                },
            )))
    }

    fn render_secret_row(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        h_flex()
            .gap(tokens.spacing.sm)
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.secret).w_full()),
            )
            .child(
                button(
                    "webhook-secret-regenerate",
                    self.t("webhookRegenerate"),
                    ButtonVariant::Secondary,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    let secret = generate_secret();
                    this.secret
                        .update(cx, |input, cx| input.set_value(secret, window, cx));
                    this.copied = false;
                    cx.notify();
                })),
            )
            .child(
                button(
                    "webhook-secret-copy",
                    self.t(if self.copied {
                        "webhookCopied"
                    } else {
                        "webhookCopy"
                    }),
                    ButtonVariant::Secondary,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_secret(cx))),
            )
    }

    fn render_advanced(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let open = self.advanced_open;
        let mut column = v_flex().gap(tokens.spacing.sm).child(
            h_flex()
                .id("webhook-advanced-toggle")
                .gap(tokens.spacing.xs)
                .items_center()
                .py(tokens.spacing.xs)
                .cursor_pointer()
                .text_color(tokens.colors.muted_foreground)
                .child(
                    Icon::new(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(px(14.)),
                )
                .child(div().text_xs().child(self.t("webhookAdvanced")))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.advanced_open = !this.advanced_open;
                    cx.notify();
                })),
        );
        if !open {
            return column;
        }
        column = column
            .child(self.render_headers(cx))
            .child(self.render_template(cx))
            .child(self.render_switch_row(
                "webhook-sign",
                "webhookFieldSign",
                "webhookSignDesc",
                self.sign_enabled,
                |this, enabled, window, cx| {
                    this.sign_enabled = enabled;
                    if enabled && this.secret.read(cx).value().trim().is_empty() {
                        // 开启签名时给一个够长够随机的起点；用户可随时改成自己的。
                        let secret = generate_secret();
                        this.secret
                            .update(cx, |input, cx| input.set_value(secret, window, cx));
                    }
                },
                cx,
            ));
        if self.sign_enabled {
            column = column.child(self.render_secret_row(cx));
        }
        column
            .child(self.render_switch_row(
                "webhook-allow-http",
                "webhookFieldAllowHttp",
                "webhookAllowHttpDesc",
                self.allow_http,
                |this, enabled, _, _| this.allow_http = enabled,
                cx,
            ))
            .child(self.render_switch_row(
                "webhook-use-proxy",
                "webhookFieldUseProxy",
                "webhookUseProxyDesc",
                self.use_proxy,
                |this, enabled, _, _| this.use_proxy = enabled,
                cx,
            ))
    }

    fn render_form(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .w_full()
            .gap(tokens.spacing.lg)
            .pr(tokens.spacing.md)
            .child(
                v_flex()
                    .gap(tokens.spacing.sm)
                    .child(self.label("webhookFieldPreset", cx))
                    .child(self.render_preset_grid(cx)),
            )
            .child(
                h_flex()
                    .gap(tokens.spacing.md)
                    .items_start()
                    .child(
                        v_flex()
                            .w(px(170.))
                            .gap(tokens.spacing.xs)
                            .child(self.label("webhookFieldName", cx))
                            .child(Input::new(&self.name).w_full()),
                    )
                    .child(div().flex_1().min_w_0().child(self.render_url_field(cx))),
            )
            .child(self.render_events(cx))
            .child(self.render_queue_filter(cx))
            .child(self.render_advanced(cx))
    }

    fn preview_body(&self, preset: Option<&WebhookPresetDto>, cx: &App) -> String {
        let own = self.template.read(cx).value();
        let template = if own.trim().is_empty() {
            preset
                .map(|preset| preset.default_template.clone())
                .unwrap_or_default()
        } else {
            own.to_string()
        };
        if template.is_empty() {
            return envelope_preview();
        }
        let is_form =
            preset.is_some_and(|preset| preset.content_type.starts_with("application/x-www-form"));
        let rendered = render_preview(&template, is_form);
        if is_form {
            return rendered;
        }
        // 渲染结果若是合法 JSON 就美化一下，方便扫读；不是就原样显示。
        serde_json::from_str::<Value>(&rendered)
            .ok()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
            .unwrap_or(rendered)
    }

    fn render_preview(&self, preset: Option<&WebhookPresetDto>, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let url = {
            let raw = self.url.read(cx).value();
            if raw.trim().is_empty() {
                preset
                    .map(|preset| preset.url_placeholder.clone())
                    .unwrap_or_default()
            } else {
                raw.trim().to_owned()
            }
        };
        let first_event = WEBHOOK_EVENTS
            .iter()
            .map(|(wire, _)| *wire)
            .find(|wire| self.events.contains(*wire))
            .unwrap_or("task.completed");
        let content_type = preset.map_or("application/json", |preset| preset.content_type.as_str());
        let mut lines = vec![
            format!("POST {url}"),
            format!("Content-Type: {content_type}"),
            format!("X-FluxDown-Event: {first_event}"),
            "X-FluxDown-Delivery: 5f2a91c7-…".to_owned(),
        ];
        if self.sign_enabled {
            lines.push("X-FluxDown-Signature: t=1789647128,v1=9c41f2…".to_owned());
        }
        lines.push("─".repeat(28));
        lines.extend(self.preview_body(preset, cx).lines().map(str::to_owned));
        let mut body = v_flex()
            .w_full()
            .font_family(tokens.typography.mono.clone())
            .text_xs()
            .text_color(tokens.colors.foreground);
        for line in lines {
            body = body.child(div().whitespace_normal().child(SharedString::from(line)));
        }
        v_flex()
            .h_full()
            .gap(tokens.spacing.sm)
            .child(
                div()
                    .text_xs()
                    .font_weight(tokens.typography.md.weight)
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t("webhookPreviewTitle")),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .rounded(tokens.radius.md)
                    .border_1()
                    .border_color(tokens.colors.border)
                    .bg(tokens.colors.surface)
                    .px(tokens.spacing.sm)
                    .py(tokens.spacing.sm)
                    .child(body.overflow_y_scrollbar()),
            )
            .child(self.hint(self.t("webhookPreviewMeta"), cx))
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let can_test = !self.testing && !self.url.read(cx).value().trim().is_empty();
        let can_save = self.can_save(cx);
        let mut row = h_flex()
            .w_full()
            .items_center()
            .gap(tokens.spacing.sm)
            .child(
                button(
                    "webhook-send-test",
                    self.t(if self.testing {
                        "webhookTesting"
                    } else {
                        "webhookSendTest"
                    }),
                    ButtonVariant::Secondary,
                    cx,
                )
                .disabled(!can_test)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send_test(cx))),
            );
        if let Some(outcome) = &self.test_result {
            row = row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(if outcome.success {
                        tokens.colors.primary
                    } else {
                        tokens.colors.destructive
                    })
                    .child(outcome.text.clone()),
            );
        } else {
            row = row.child(div().flex_1());
        }
        row.child(
            button(
                "webhook-dialog-cancel",
                self.t("cancel"),
                ButtonVariant::Ghost,
                cx,
            )
            .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            button(
                "webhook-dialog-save",
                self.t("webhookSaveEndpoint"),
                ButtonVariant::Primary,
                cx,
            )
            .disabled(!can_save)
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.save(window, cx))),
        )
    }
}

/// 占位符只在变化时写回，避免每帧 notify 输入框。
fn sync_placeholder(
    input: &Entity<InputState>,
    placeholder: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    if *input.read(cx).presentation().placeholder() != placeholder {
        input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
    }
}

impl Render for WebhookDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let preset = self.current_preset().cloned();
        // 占位符随预设变化（名称 = 预设名，URL = 预设示例）。
        let name_hint = SharedString::from(
            preset
                .as_ref()
                .map(|preset| preset.label.clone())
                .unwrap_or_default(),
        );
        let url_hint = SharedString::from(
            preset
                .as_ref()
                .map(|preset| preset.url_placeholder.clone())
                .unwrap_or_default(),
        );
        sync_placeholder(&self.name, name_hint, window, cx);
        sync_placeholder(&self.url, url_hint, window, cx);
        v_flex()
            .w_full()
            .h(px(440.))
            .gap(tokens.spacing.md)
            .child(self.hint(self.t("webhookDialogDesc"), cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .gap(tokens.spacing.md)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.render_form(cx).overflow_y_scrollbar()),
                    )
                    .child(div().w(px(1.)).bg(tokens.colors.border))
                    .child(
                        div()
                            .w(px(300.))
                            .child(self.render_preview(preset.as_ref(), cx)),
                    ),
            )
            .child(div().h(px(1.)).w_full().bg(tokens.colors.border))
            .child(self.render_footer(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_placeholders_like_dart() {
        assert_eq!(
            render_preview(
                r#"{"text":"{task.fileName} · {task.totalBytesHuman}"}"#,
                false
            ),
            r#"{"text":"ubuntu-24.04.2-desktop-amd64.iso · 6.0 GB"}"#
        );
        assert_eq!(render_preview("{unknown} {{ {", false), "{unknown} {{ {");
        assert_eq!(
            render_preview("q={event.summary}", true),
            "q=ubuntu-24.04.2-desktop-amd64.iso+%C2%B7+6.0+GB"
        );
        assert_eq!(
            render_preview("\"{event.title}\"", false),
            "\"Download completed\""
        );
    }

    #[test]
    fn form_encoding_matches_dart() {
        assert_eq!(form_encode("a b-_.!~*'()"), "a+b-_.!~*'()");
        assert_eq!(form_encode("/?&="), "%2F%3F%26%3D");
    }

    #[test]
    fn url_validation() {
        assert_eq!(url_error_key("", false), None);
        assert_eq!(url_error_key("https://ntfy.sh/topic", false), None);
        assert_eq!(
            url_error_key("http://192.168.1.2:8123/api", false),
            Some("webhookUrlWarnHttp")
        );
        assert_eq!(url_error_key("http://192.168.1.2:8123/api", true), None);
        assert_eq!(url_error_key("ftp://host", true), Some("webhookUrlInvalid"));
        assert_eq!(
            url_error_key("https:///path", true),
            Some("webhookUrlInvalid")
        );
        assert_eq!(url_error_key("not a url", true), Some("webhookUrlInvalid"));
    }

    #[test]
    fn secret_shape() {
        let secret = generate_secret();
        assert_eq!(secret.len(), 6 + 32);
        assert!(secret.starts_with("whsec_"));
        assert!(secret[6..].chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(generate_secret(), secret);
    }

    #[test]
    fn envelope_is_valid_json() {
        let value: Value = serde_json::from_str(&envelope_preview()).expect("json");
        assert_eq!(value["task"]["totalBytes"], json!(6_442_450_944_i64));
    }
}
