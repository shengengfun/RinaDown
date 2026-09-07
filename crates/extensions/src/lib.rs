//! 插件、插件市场与受管组件（ffmpeg / yt-dlp）capability。

mod components;
mod controller;
mod pages;

use std::{future::Future, pin::Pin, sync::Arc};

use fluxdown_protocol::{AgentSnapshot, ApplicationErrorCode, RpcErrorData, ServiceEvent};
use fluxdown_ui_i18n::Translator;
use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, WindowExt as _,
    notification::Notification,
    tab::{Tab, TabBar},
    v_flex,
};

use controller::{COMPONENT_KINDS, component_slot};
pub use controller::{ExtensionsController, ExtensionsSignal};
use pages::{managed_components::ComponentUi, plugins::PluginsUi};

pub type PortFuture<T> = Pin<Box<dyn Future<Output = Result<T, RpcErrorData>> + Send + 'static>>;

pub trait ExtensionsPort: Send + Sync {
    fn call(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> PortFuture<serde_json::Value>;
}

/// 扩展分类的子页（与 Flutter `extensions→[plugins, components]` 一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionsTab {
    Plugins,
    Components,
}

impl ExtensionsTab {
    const ALL: [Self; 2] = [Self::Plugins, Self::Components];

    fn index(self) -> usize {
        match self {
            Self::Plugins => 0,
            Self::Components => 1,
        }
    }
}

pub struct ExtensionsView {
    translator: Entity<Translator>,
    controller: ExtensionsController,
    tab: ExtensionsTab,
    last_error: Option<String>,
    /// 事件回调没有窗口，提示在下一帧渲染时经 `Window::defer` 投递。
    pending_notices: Vec<String>,
    plugins: PluginsUi,
    components: [ComponentUi; 2],
}

impl ExtensionsView {
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn ExtensionsPort>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        Self {
            translator,
            controller: ExtensionsController::new(port),
            tab: ExtensionsTab::Plugins,
            last_error: None,
            pending_notices: Vec::new(),
            plugins: PluginsUi::default(),
            components: [ComponentUi::default(), ComponentUi::default()],
        }
    }

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>) {
        self.last_error = None;
        self.controller.replace_snapshot(snapshot);
        cx.notify();
    }

    pub fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
        match self.controller.apply_event(event) {
            Some(ExtensionsSignal::ComponentProgress {
                kind,
                downloaded_bytes,
                total_bytes,
            }) => {
                let ui = &mut self.components[component_slot(kind)];
                ui.installing = true;
                ui.downloaded_bytes = downloaded_bytes;
                ui.total_bytes = total_bytes;
            }
            Some(ExtensionsSignal::ComponentResult { kind, ok, message }) => {
                let ui = &mut self.components[component_slot(kind)];
                ui.last_result = Some((ok, message));
                // 非本视图发起的安装（如 Web UI）：结果到达即结束进度展示。
                if !ui.install_pending {
                    ui.installing = false;
                }
            }
            Some(ExtensionsSignal::PluginAutoDisabled { name }) => {
                let notice = self
                    .translator
                    .read(cx)
                    .text_with("pluginAutoDisabledToast", &[("name", &name)]);
                self.pending_notices.push(notice);
            }
            None => {}
        }
        if !self.controller.is_stale() {
            self.last_error = None;
        }
        cx.notify();
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.last_error = Some(
            self.translator
                .read(cx)
                .text("localServiceDisconnected")
                .to_owned(),
        );
        self.controller.mark_stale();
        cx.notify();
    }

    pub(crate) fn show_tab(&mut self, tab: ExtensionsTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
    }

    pub(crate) fn toast_success(
        &self,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.push_notification(Notification::success(message), cx);
    }

    pub(crate) fn toast_error(&self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        window.push_notification(Notification::error(message).autohide(false), cx);
    }
}

/// agent 端口只回传错误码（服务端 message 不透传），按码映射通用文案。
pub(crate) fn error_text(translator: &Translator, error: &RpcErrorData) -> String {
    let key = match error.code {
        ApplicationErrorCode::Unavailable | ApplicationErrorCode::Timeout => {
            "localServiceDisconnected"
        }
        ApplicationErrorCode::InvalidArgument | ApplicationErrorCode::NotFound => {
            "localServiceInvalidArgument"
        }
        ApplicationErrorCode::Conflict => "localServiceConflict",
        ApplicationErrorCode::Unsupported => "settingsUnsupportedOnPlatform",
        ApplicationErrorCode::ProtocolIncompatible
        | ApplicationErrorCode::Unauthorized
        | ApplicationErrorCode::Cancelled
        | ApplicationErrorCode::Internal => "localServiceActionFailed",
    };
    translator.text(key).to_owned()
}

impl Render for ExtensionsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab;
        if !self.pending_notices.is_empty() {
            let notices = std::mem::take(&mut self.pending_notices);
            window.defer(cx, move |window, cx| {
                for notice in notices {
                    window.push_notification(Notification::warning(notice), cx);
                }
            });
        }
        let body = match tab {
            ExtensionsTab::Plugins => self.render_plugins(window, cx).into_any_element(),
            ExtensionsTab::Components => self.render_components(window, cx).into_any_element(),
        };
        let translator = self.translator.read(cx);
        let labels = [
            translator.text("settingsCatPlugins").to_owned(),
            translator.text("settingsCatComponents").to_owned(),
        ];
        let danger = cx.theme().danger;
        v_flex()
            .w_full()
            .gap_4()
            .when_some(self.last_error.clone(), |this, error| {
                this.child(div().text_sm().text_color(danger).child(error))
            })
            .child(
                TabBar::new("extensions-tabs")
                    .underline()
                    .selected_index(tab.index())
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        if let Some(tab) = ExtensionsTab::ALL.get(*index) {
                            this.show_tab(*tab, cx);
                        }
                    }))
                    .children(labels.into_iter().map(|label| Tab::new().label(label))),
            )
            .child(body)
    }
}

/// 组件 UI 状态数组的固定顺序与 [`COMPONENT_KINDS`] 对齐。
const _: () = assert!(COMPONENT_KINDS.len() == 2);
