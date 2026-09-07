//! GPUI 账户、认证、设备、订单与推介 capability。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fluxdown_ui_i18n::Translator;
use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputContentType, InputState},
    v_flex,
};

pub type PortFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, fluxdown_protocol::RpcErrorData>> + Send + 'static>>;

pub enum AccountCommand {
    Auth {
        method: &'static str,
        params: serde_json::Value,
    },
    Profile {
        method: &'static str,
        params: serde_json::Value,
    },
    Device {
        method: &'static str,
        params: serde_json::Value,
    },
    Plan {
        method: &'static str,
        params: serde_json::Value,
    },
    Order {
        method: &'static str,
        params: serde_json::Value,
    },
    Referral {
        method: &'static str,
        params: serde_json::Value,
    },
}

pub trait AccountPort: Send + Sync {
    fn execute(&self, command: AccountCommand) -> PortFuture<serde_json::Value>;
}

pub struct AccountController {
    port: Arc<dyn AccountPort>,
    session: Option<fluxdown_protocol::AgentSessionDto>,
    devices: Vec<fluxdown_protocol::CloudDevice>,
    stale: bool,
}

impl AccountController {
    #[must_use]
    pub fn new(port: Arc<dyn AccountPort>) -> Self {
        Self {
            port,
            session: None,
            devices: Vec::new(),
            stale: true,
        }
    }

    pub fn replace_snapshot(&mut self, snapshot: &fluxdown_protocol::AgentSnapshot) {
        self.session.clone_from(&snapshot.session);
        self.devices.clone_from(&snapshot.cloud_devices);
        self.stale = false;
    }

    pub fn apply_event(&mut self, event: &fluxdown_protocol::ServiceEvent) {
        let fluxdown_protocol::ServiceEvent::Agent(event) = event else {
            return;
        };
        match event {
            fluxdown_protocol::AgentEvent::SessionChanged(session) => {
                self.session.clone_from(session.as_ref())
            }
            fluxdown_protocol::AgentEvent::CloudDevicesChanged(devices) => {
                self.devices.clone_from(devices)
            }
            _ => {}
        }
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }
    #[must_use]
    pub fn session(&self) -> Option<&fluxdown_protocol::AgentSessionDto> {
        self.session.as_ref()
    }
    #[must_use]
    pub fn devices(&self) -> &[fluxdown_protocol::CloudDevice] {
        &self.devices
    }
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.stale
    }
}

pub struct AccountView {
    translator: Entity<Translator>,
    controller: AccountController,
    account_input: Entity<InputState>,
    password_input: Entity<InputState>,
    code_input: Entity<InputState>,
    verification_required: bool,
    last_error: Option<String>,
}

impl AccountView {
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn AccountPort>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        let account_placeholder = translator
            .read(cx)
            .text("accountLoginAccountPlaceholder")
            .to_owned();
        let password_placeholder = translator
            .read(cx)
            .text("accountPasswordPlaceholder")
            .to_owned();
        let code_placeholder = translator
            .read(cx)
            .text("accountCodePlaceholder")
            .to_owned();
        Self {
            translator,
            controller: AccountController::new(port),
            account_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(account_placeholder)),
            password_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(password_placeholder)
            }),
            code_input: cx.new(|cx| InputState::new(window, cx).placeholder(code_placeholder)),
            verification_required: false,
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

    fn submit_login(&mut self, verify: bool, cx: &mut Context<Self>) {
        if self.controller.is_stale() {
            return;
        }
        let account = self.account_input.read(cx).value().trim().to_owned();
        let password = self.password_input.read(cx).value().to_string();
        let code = self.code_input.read(cx).value().trim().to_owned();
        if account.is_empty() || password.is_empty() || (verify && code.is_empty()) {
            return;
        }
        let params = if verify {
            serde_json::json!({"account": account, "password": password, "code": code})
        } else {
            serde_json::json!({"account": account, "password": password})
        };
        let method = if verify {
            fluxdown_protocol::method::AGENT_AUTH_LOGIN_VERIFY
        } else {
            fluxdown_protocol::method::AGENT_AUTH_LOGIN
        };
        let future = self
            .controller
            .port
            .execute(AccountCommand::Auth { method, params });
        cx.spawn(async move |this, cx| {
            let result = future.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(value) => {
                        this.last_error = None;
                        this.verification_required =
                            serde_json::from_value::<fluxdown_protocol::AgentLoginResult>(value)
                                .is_ok_and(|result| {
                                    matches!(
                                result,
                                fluxdown_protocol::AgentLoginResult::DeviceVerificationRequired {
                                    ..
                                }
                            )
                                });
                    }
                    Err(_) => {
                        this.last_error = Some(
                            this.translator
                                .read(cx)
                                .text("localServiceActionFailed")
                                .to_owned(),
                        )
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn spawn_action(&mut self, future: PortFuture<serde_json::Value>, cx: &mut Context<Self>) {
        self.last_error = None;
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
    }
}

impl Render for AccountView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let translator = self.translator.read(cx);
        let tokens = cx.theme().clone();
        let title = translator.text("settingsCatAccount").to_owned();
        let subtitle = translator.text("accountHeroSubtitle").to_owned();
        let login = translator.text("accountLogin").to_owned();
        let logout = translator.text("accountLogout").to_owned();
        let verify = translator.text("confirm").to_owned();
        let devices_title = translator.text("accountDevicesTitle").to_owned();
        let retry = translator.text("accountDevicesRetry").to_owned();
        let read_only = self.controller.is_stale();
        let session = self.controller.session().cloned();
        let devices = self.controller.devices().to_vec();
        v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .child(div().text_lg().font_semibold().child(title))
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.muted_foreground)
                    .child(subtitle),
            )
            .when(session.is_none(), |this| {
                this.child(Input::new(&self.account_input).small())
                    .child(
                        Input::new(&self.password_input)
                            .small()
                            .content_type(InputContentType::Password)
                            .mask_toggle(),
                    )
                    .when(self.verification_required, |this| {
                        this.child(Input::new(&self.code_input).small())
                    })
                    .child(
                        Button::new("account-login")
                            .primary()
                            .small()
                            .label(if self.verification_required {
                                verify
                            } else {
                                login
                            })
                            .disabled(read_only)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.submit_login(this.verification_required, cx);
                            })),
                    )
            })
            .when_some(self.last_error.clone(), |this, error| {
                this.child(div().text_sm().child(error))
            })
            .when_some(session, |this, session| {
                this.child(
                    v_flex()
                        .gap_3()
                        .child(div().text_sm().font_semibold().child(session.user.nickname))
                        .child(
                            div()
                                .text_sm()
                                .text_color(tokens.muted_foreground)
                                .child(session.user.email),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("account-devices-refresh")
                                        .outline()
                                        .small()
                                        .label(retry)
                                        .disabled(read_only)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let future =
                                                this.controller.port.execute(AccountCommand::Device {
                                                    method: fluxdown_protocol::method::AGENT_DEVICE_LIST,
                                                    params: serde_json::json!({}),
                                                });
                                            this.spawn_action(future, cx);
                                        })),
                                )
                                .child(
                                    Button::new("account-logout")
                                        .outline()
                                        .small()
                                        .label(logout)
                                        .disabled(read_only)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let future =
                                                this.controller.port.execute(AccountCommand::Auth {
                                                    method: fluxdown_protocol::method::AGENT_AUTH_LOGOUT,
                                                    params: serde_json::json!({}),
                                                });
                                            this.spawn_action(future, cx);
                                        })),
                                ),
                        )
                        .child(div().text_sm().font_semibold().child(devices_title))
                        .children(devices.into_iter().map(|device| {
                            h_flex()
                                .w_full()
                                .justify_between()
                                .child(device.name)
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(tokens.muted_foreground)
                                        .child(device.platform.unwrap_or_default()),
                                )
                        })),
                )
            })
    }
}
