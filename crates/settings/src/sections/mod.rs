//! 设置分区：每个模块产出一个 [`SettingPage`]。
//!
//! 页面闭包只捕获 `Entity<SettingsStore>` 与预先解析好的文案；
//! 所有键名、范围、枚举值以 `fluxdown_protocol` 目录为准。

pub(crate) mod about;
pub(crate) mod api;
pub(crate) mod appearance;
pub(crate) mod bt;
pub(crate) mod categories;
pub(crate) mod category_dialog;
pub(crate) mod doctor;
pub(crate) mod download;
pub(crate) mod ed2k;
pub(crate) mod general;
pub(crate) mod notify;
pub(crate) mod proxy;
pub(crate) mod site_auth;
pub(crate) mod subscription;
pub(crate) mod user_agent;
pub(crate) mod webhook;
pub(crate) mod webhook_dialog;

use fluxdown_protocol::{DaemonConfigKind, daemon_config_field};
use fluxdown_ui_i18n::Translator;
use gpui::{AnyView, App, Entity, IntoElement as _, ParentElement as _, SharedString};
use gpui_component::{
    Icon, IconName,
    setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage},
};

use crate::store::SettingsStore;

/// 分区构建上下文：存储实体 + 当前语言的翻译快照 + 翻译实体 + app 注入的内容槽。
pub(crate) struct SectionContext<'a> {
    pub store: &'a Entity<SettingsStore>,
    pub translator: &'a Translator,
    pub translator_entity: &'a Entity<Translator>,
}

impl SectionContext<'_> {
    /// 翻译文案；键缺失时回退为键名（与 Flutter 基线一致）。
    pub(crate) fn t(&self, key: &str) -> SharedString {
        SharedString::from(self.translator.text(key).to_owned())
    }

    #[must_use]
    pub(crate) fn store(&self) -> Entity<SettingsStore> {
        self.store.clone()
    }

    // ───────────────────────── daemon 字段 ─────────────────────────

    pub(crate) fn daemon_switch(&self, key: &'static str) -> SettingField<bool> {
        let get = self.store();
        let set = self.store();
        SettingField::switch(
            move |cx: &App| get.read(cx).daemon_bool(key),
            move |value, cx: &mut App| {
                set.update(cx, |store, cx| store.set_daemon_bool(key, value, cx));
            },
        )
        .default_value(default_bool(key))
    }

    pub(crate) fn daemon_number(&self, key: &'static str) -> SettingField<f64> {
        self.daemon_number_with(key, 1.0)
    }

    pub(crate) fn daemon_number_with(&self, key: &'static str, step: f64) -> SettingField<f64> {
        let get = self.store();
        let set = self.store();
        let (min, max, float) = match daemon_config_field(key).map(|field| field.kind) {
            Some(DaemonConfigKind::Integer { min, max }) => (min as f64, max as f64, false),
            Some(DaemonConfigKind::Float { min }) => (min, f64::MAX, true),
            _ => (f64::MIN, f64::MAX, false),
        };
        SettingField::number_input(
            NumberFieldOptions { min, max, step },
            move |cx: &App| {
                if float {
                    get.read(cx).daemon_f64(key)
                } else {
                    get.read(cx).daemon_i64(key) as f64
                }
            },
            move |value, cx: &mut App| {
                set.update(cx, |store, cx| {
                    if float {
                        store.set_daemon_f64(key, value, cx);
                    } else {
                        store.set_daemon_i64(key, value.round() as i64, cx);
                    }
                });
            },
        )
        .default_value(default_f64(key))
    }

    pub(crate) fn daemon_input(&self, key: &'static str) -> SettingField<SharedString> {
        let get = self.store();
        let set = self.store();
        SettingField::input(
            move |cx: &App| SharedString::from(get.read(cx).daemon_str(key)),
            move |value: SharedString, cx: &mut App| {
                set.update(cx, |store, cx| store.set_daemon(key, value.to_string(), cx));
            },
        )
        .default_value(SharedString::from(
            fluxdown_protocol::daemon_config_default(key),
        ))
    }

    /// 枚举下拉：`options` 为 `(wire 值, 文案)`；默认取协议目录。
    pub(crate) fn daemon_dropdown(
        &self,
        key: &'static str,
        options: Vec<(SharedString, SharedString)>,
    ) -> SettingField<SharedString> {
        let get = self.store();
        let set = self.store();
        SettingField::dropdown(
            options,
            move |cx: &App| SharedString::from(get.read(cx).daemon_str(key)),
            move |value: SharedString, cx: &mut App| {
                set.update(cx, |store, cx| store.set_daemon(key, value.to_string(), cx));
            },
        )
        .default_value(SharedString::from(
            fluxdown_protocol::daemon_config_default(key),
        ))
    }

    /// 由协议目录的枚举值生成下拉，文案键为 `{prefix}{Value}`（首字母大写驼峰）。
    pub(crate) fn daemon_enum_dropdown(
        &self,
        key: &'static str,
        label_prefix: &str,
    ) -> SettingField<SharedString> {
        let values: &[&str] = match daemon_config_field(key).map(|field| field.kind) {
            Some(DaemonConfigKind::Enum(values)) => values,
            _ => &[],
        };
        let options = values
            .iter()
            .map(|value| {
                (
                    SharedString::from(*value),
                    self.t(&format!("{label_prefix}{}", camel(value))),
                )
            })
            .collect();
        self.daemon_dropdown(key, options)
    }

    // ───────────────────────── 偏好字段 ─────────────────────────

    pub(crate) fn pref_switch(&self, key: &'static str, default: bool) -> SettingField<bool> {
        let get = self.store();
        let set = self.store();
        SettingField::switch(
            move |cx: &App| get.read(cx).pref_bool(key, default),
            move |value, cx: &mut App| {
                set.update(cx, |store, cx| store.set_pref_bool(key, value, cx));
            },
        )
        .default_value(default)
    }

    pub(crate) fn pref_input(
        &self,
        key: &'static str,
        default: &'static str,
    ) -> SettingField<SharedString> {
        let get = self.store();
        let set = self.store();
        SettingField::input(
            move |cx: &App| SharedString::from(get.read(cx).pref_str(key, default)),
            move |value: SharedString, cx: &mut App| {
                set.update(cx, |store, cx| {
                    store.set_pref_str(key, value.to_string(), cx)
                });
            },
        )
        .default_value(SharedString::from(default))
    }

    pub(crate) fn pref_number(
        &self,
        key: &'static str,
        default: i64,
        min: i64,
        max: i64,
    ) -> SettingField<f64> {
        let get = self.store();
        let set = self.store();
        SettingField::number_input(
            NumberFieldOptions {
                min: min as f64,
                max: max as f64,
                step: 1.0,
            },
            move |cx: &App| get.read(cx).pref_i64(key, default) as f64,
            move |value, cx: &mut App| {
                set.update(cx, |store, cx| {
                    store.set_pref_i64(key, value.round() as i64, cx);
                });
            },
        )
        .default_value(default as f64)
    }

    pub(crate) fn pref_dropdown(
        &self,
        key: &'static str,
        default: &'static str,
        options: Vec<(SharedString, SharedString)>,
    ) -> SettingField<SharedString> {
        let get = self.store();
        let set = self.store();
        SettingField::dropdown(
            options,
            move |cx: &App| SharedString::from(get.read(cx).pref_str(key, default)),
            move |value: SharedString, cx: &mut App| {
                set.update(cx, |store, cx| {
                    store.set_pref_str(key, value.to_string(), cx)
                });
            },
        )
        .default_value(SharedString::from(default))
    }

    // ───────────────────────── 条目 ─────────────────────────

    /// 标题 + 可选描述的标准条目；描述键不存在时不显示描述。
    pub(crate) fn item<F>(&self, title_key: &str, desc_key: Option<&str>, field: F) -> SettingItem
    where
        F: gpui_component::setting::AnySettingField + 'static,
    {
        let mut item = SettingItem::new(self.t(title_key), field);
        if let Some(desc_key) = desc_key
            && self.translator.text(desc_key) != desc_key
        {
            item = item.description(self.t(desc_key));
        }
        item
    }
}

fn default_bool(key: &str) -> bool {
    matches!(fluxdown_protocol::daemon_config_default(key), "true" | "1")
}

fn default_f64(key: &str) -> f64 {
    fluxdown_protocol::daemon_config_default(key)
        .parse()
        .unwrap_or(0.0)
}

/// `delete_files` → `DeleteFiles`；`or` → `Or`。
pub(crate) fn camel(value: &str) -> String {
    value
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// 由外部 capability 提供内容的整页（账户 / 扩展）；槽位为空时显示分类描述。
pub(crate) fn slot_page(
    ctx: &SectionContext,
    title_key: &str,
    desc_key: &str,
    icon: IconName,
    view: Option<AnyView>,
) -> SettingPage {
    let description = ctx.t(desc_key);
    let item = match view {
        Some(view) => SettingItem::render(move |_, _, _| view.clone().into_any_element()),
        None => SettingItem::render(move |_, _, _| {
            gpui::div().child(description.clone()).into_any_element()
        }),
    };
    SettingPage::new(ctx.t(title_key))
        .icon(Icon::new(icon))
        .description(ctx.t(desc_key))
        .resettable(false)
        .group(SettingGroup::new().item(item))
}
