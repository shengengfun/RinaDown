//! 设置窗口内容：左侧分类 + 搜索 + 右侧分区（gpui-component `Settings` DSL）。

use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyView, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Sizable as _, group_box::GroupBoxVariant, setting::Settings, v_flex};

use crate::sections::{
    self, SectionContext, about, api, appearance, bt, doctor, download, ed2k, general, notify,
    proxy,
};
use crate::store::{SettingsErrorKind, SettingsStore};

/// app 注入的外部内容槽：账户页与扩展页由对应 capability 提供。
#[derive(Default)]
pub struct SettingsContentSlots {
    pub account: Option<AnyView>,
    pub extensions: Option<AnyView>,
}

/// 设置能力的顶层页面。
pub struct SettingsView {
    store: Entity<SettingsStore>,
    translator: Entity<Translator>,
    slots: SettingsContentSlots,
}

impl SettingsView {
    /// 创建设置页面，并订阅共享翻译状态与设置存储。
    ///
    /// `store` 由 app 持有并跨窗口复用：设置窗口关闭后防抖中的写回仍会完成，
    /// 快照/事件也持续进入同一存储。
    pub fn new(
        translator: Entity<Translator>,
        store: Entity<SettingsStore>,
        slots: SettingsContentSlots,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self {
            store,
            translator,
            slots,
        }
    }

    #[must_use]
    pub fn store(&self) -> Entity<SettingsStore> {
        self.store.clone()
    }

    fn feedback(&self, cx: &Context<Self>) -> Option<(SharedString, bool)> {
        let store = self.store.read(cx);
        let translator = self.translator.read(cx);
        if let Some(error) = store.last_error() {
            let mut text = translator.text(error.kind.i18n_key()).to_owned();
            if error.kind == SettingsErrorKind::InvalidArgument && !error.detail.is_empty() {
                text.push_str(": ");
                text.push_str(&error.detail);
            }
            return Some((SharedString::from(text), true));
        }
        store
            .last_notice()
            .map(|key| (SharedString::from(translator.text(key).to_owned()), false))
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        let translator = self.translator.read(cx).clone();
        let ctx = SectionContext {
            store: &self.store,
            translator: &translator,
            translator_entity: &self.translator,
        };
        let pages = vec![
            general::page(&ctx, cx),
            sections::slot_page(
                &ctx,
                "settingsCatAccount",
                "settingsCatAccountDesc",
                gpui_component::IconName::User,
                self.slots.account.clone(),
            ),
            appearance::page(&ctx, cx),
            download::page(&ctx, cx),
            bt::page(&ctx, cx),
            ed2k::page(&ctx, cx),
            proxy::page(&ctx, cx),
            api::page(&ctx, cx),
            notify::page(&ctx, cx),
            sections::slot_page(
                &ctx,
                "settingsCatExtensions",
                "settingsCatExtensionsDesc",
                gpui_component::IconName::Settings2,
                self.slots.extensions.clone(),
            ),
            doctor::page(&ctx, cx),
            about::page(&ctx, cx),
        ];
        let feedback = self.feedback(cx);

        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(tokens.colors.background)
            .when_some(feedback, |this, (text, is_error)| {
                this.child(
                    div()
                        .w_full()
                        .px(tokens.spacing.xl)
                        .py(tokens.spacing.sm)
                        .text_size(tokens.typography.sm.size)
                        .text_color(if is_error {
                            tokens.colors.destructive
                        } else {
                            tokens.colors.muted_foreground
                        })
                        .child(text),
                )
            })
            .child(
                div().flex_1().min_h_0().child(
                    Settings::new("fluxdown-settings")
                        .sidebar_width(px(200.))
                        .with_group_variant(GroupBoxVariant::Outline)
                        .small()
                        .pages(pages),
                ),
            )
    }
}
