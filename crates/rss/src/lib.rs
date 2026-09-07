//! RSS 订阅、条目、验证与动作 capability。

use std::{future::Future, pin::Pin, sync::Arc};

use fluxdown_ui_i18n::Translator;
use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex,
};

pub type PortFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, fluxdown_protocol::RpcErrorData>> + Send + 'static>>;

pub trait RssPort: Send + Sync {
    fn call(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> PortFuture<serde_json::Value>;
}

pub struct RssController {
    port: Arc<dyn RssPort>,
    sources: Vec<fluxdown_protocol::RssSourceDto>,
    stale: bool,
}

impl RssController {
    pub fn new(port: Arc<dyn RssPort>) -> Self {
        Self {
            port,
            sources: Vec::new(),
            stale: true,
        }
    }
    pub fn replace_snapshot(&mut self, snapshot: &fluxdown_protocol::AgentSnapshot) {
        self.sources.clone_from(&snapshot.daemon.rss_sources);
        self.stale = false;
    }
    pub fn apply_event(&mut self, event: &fluxdown_protocol::ServiceEvent) {
        let fluxdown_protocol::ServiceEvent::Agent(event) = event else {
            return;
        };
        match event {
            fluxdown_protocol::AgentEvent::DaemonSnapshotReplaced(snapshot) => {
                self.sources.clone_from(&snapshot.rss_sources);
                self.stale = false;
            }
            fluxdown_protocol::AgentEvent::DaemonConnectionChanged(connected) => {
                self.stale = !connected;
            }
            fluxdown_protocol::AgentEvent::Daemon(fluxdown_protocol::DaemonEvent::Engine(
                fluxdown_protocol::WsServerMsg::RssSourcesChanged { sources },
            )) => self.sources.clone_from(sources),
            _ => {}
        }
    }
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }
    pub fn sources(&self) -> &[fluxdown_protocol::RssSourceDto] {
        &self.sources
    }
    pub fn is_stale(&self) -> bool {
        self.stale
    }
    pub fn create(&self, url: String) -> PortFuture<serde_json::Value> {
        if self.stale {
            return unavailable();
        }
        self.port.call(
            fluxdown_protocol::method::DAEMON_RSS_CREATE_SOURCE,
            serde_json::json!({"url": url, "enabled": true, "autoDownload": false}),
        )
    }
    pub fn delete(&self, source_id: String) -> PortFuture<serde_json::Value> {
        if self.stale {
            return unavailable();
        }
        self.port.call(
            fluxdown_protocol::method::DAEMON_RSS_DELETE_SOURCE,
            serde_json::json!({"sourceId": source_id}),
        )
    }
    pub fn refresh(&self, source_id: String) -> PortFuture<serde_json::Value> {
        if self.stale {
            return Box::pin(async {
                Err(fluxdown_protocol::RpcErrorData::new(
                    fluxdown_protocol::ApplicationErrorCode::Unavailable,
                    true,
                ))
            });
        }
        self.port.call(
            fluxdown_protocol::method::DAEMON_RSS_REFRESH_SOURCE,
            serde_json::json!({"sourceId": source_id}),
        )
    }
}

fn unavailable() -> PortFuture<serde_json::Value> {
    Box::pin(async {
        Err(fluxdown_protocol::RpcErrorData::new(
            fluxdown_protocol::ApplicationErrorCode::Unavailable,
            true,
        ))
    })
}

pub struct RssView {
    translator: Entity<Translator>,
    controller: RssController,
    url_input: Entity<InputState>,
    last_error: Option<String>,
}

impl RssView {
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn RssPort>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        let placeholder = translator.read(cx).text("urlPlaceholder").to_owned();
        Self {
            translator,
            controller: RssController::new(port),
            url_input: cx.new(|cx| InputState::new(window, cx).placeholder(placeholder)),
            last_error: None,
        }
    }
    pub fn replace_snapshot(
        &mut self,
        snapshot: &fluxdown_protocol::AgentSnapshot,
        cx: &mut Context<Self>,
    ) {
        self.last_error = None;
        self.controller.replace_snapshot(snapshot);
        cx.notify();
    }
    pub fn apply_event(&mut self, event: &fluxdown_protocol::ServiceEvent, cx: &mut Context<Self>) {
        self.controller.apply_event(event);
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
}

impl Render for RssView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let add_label = self.translator.read(cx).text("rssAddSource").to_owned();
        let refresh_label = self.translator.read(cx).text("rssRefreshNow").to_owned();
        let delete_label = self.translator.read(cx).text("delete").to_owned();
        let stale = self.controller.is_stale();
        let rows = self.controller.sources().to_vec();
        v_flex()
            .size_full()
            .gap_3()
            .p_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.url_input).small()))
                    .child(
                        Button::new("rss-add-source")
                            .primary()
                            .small()
                            .label(add_label)
                            .disabled(stale)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let url = this.url_input.read(cx).value().trim().to_owned();
                                if url.is_empty() {
                                    return;
                                }
                                this.url_input
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                let future = this.controller.create(url);
                                cx.spawn(async move |this, cx| {
                                    let result = future.await;
                                    let _ = this.update(cx, |this, cx| {
                                        this.last_error = result.err().map(|_| {
                                            this.translator
                                                .read(cx)
                                                .text("localServiceActionFailed")
                                                .to_owned()
                                        });
                                        cx.notify();
                                    });
                                })
                                .detach();
                            })),
                    ),
            )
            .when_some(self.last_error.clone(), |this, error| {
                this.child(div().text_sm().child(error))
            })
            .children(rows.into_iter().map(|source| {
                let refresh_id = source.source_id.clone();
                let delete_id = source.source_id.clone();
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .py_2()
                    .border_b_1()
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(div().text_sm().child(source.name))
                            .child(div().text_xs().truncate().child(source.url)),
                    )
                    .child(
                        Button::new(format!("rss-refresh-{refresh_id}"))
                            .outline()
                            .small()
                            .label(refresh_label.clone())
                            .disabled(stale)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let future = this.controller.refresh(refresh_id.clone());
                                cx.spawn(async move |this, cx| {
                                    let failed = future.await.is_err();
                                    let _ = this.update(cx, |this, cx| {
                                        this.last_error = failed.then(|| {
                                            this.translator
                                                .read(cx)
                                                .text("localServiceActionFailed")
                                                .to_owned()
                                        });
                                        cx.notify();
                                    });
                                })
                                .detach();
                            })),
                    )
                    .child(
                        Button::new(format!("rss-delete-{delete_id}"))
                            .danger()
                            .small()
                            .label(delete_label.clone())
                            .disabled(stale)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let future = this.controller.delete(delete_id.clone());
                                cx.spawn(async move |this, cx| {
                                    let failed = future.await.is_err();
                                    let _ = this.update(cx, |this, cx| {
                                        this.last_error = failed.then(|| {
                                            this.translator
                                                .read(cx)
                                                .text("localServiceActionFailed")
                                                .to_owned()
                                        });
                                        cx.notify();
                                    });
                                })
                                .detach();
                            })),
                    )
            }))
    }
}
