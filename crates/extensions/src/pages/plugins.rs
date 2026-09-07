//! 插件子页：已安装插件管理（启用 / 设置 / 卸载）+ 安装区（zip / 开发目录）
//! + 插件市场浏览与安装。

use std::collections::HashSet;

use fluxdown_protocol::{InstalledPlugin, MarketEntryDto, PluginDto};
use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::{DialogButtonProps, DialogFooter},
    h_flex,
    input::{Input, InputEvent, InputState},
    link::Link,
    switch::Switch,
    tag::Tag,
    v_flex,
};

use super::Frame;
use crate::{
    ExtensionsTab, ExtensionsView,
    components::{
        plugin_detail::{PluginDetail, open_plugin_detail, yanked_label},
        plugin_settings::PluginSettingsForm,
    },
    controller::{COMPONENT_KINDS, component_wire_name},
    error_text,
};

/// 市场列表每次展开的条数。
pub const MARKET_PAGE_SIZE: usize = 50;

/// 插件写操作类型；结果提示按此分流。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginOp {
    Install,
    Uninstall,
    SetEnabled,
}

pub(crate) struct PluginsUi {
    pub dev_mode: bool,
    pub dev_dir: String,
    pub installing_file: bool,
    pub installing_dir: bool,
    /// 有写操作在途的插件标识（禁用其卡片上的控件）。
    pub busy: HashSet<String>,
    pub market: MarketUi,
}

impl Default for PluginsUi {
    fn default() -> Self {
        Self {
            dev_mode: true,
            dev_dir: String::new(),
            installing_file: false,
            installing_dir: false,
            busy: HashSet::new(),
            market: MarketUi::default(),
        }
    }
}

pub(crate) struct MarketUi {
    pub requested: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub entries: Vec<MarketEntryDto>,
    pub search: Option<Entity<InputState>>,
    pub limit: usize,
    /// 安装在途的市场插件 id。
    pub pending: HashSet<String>,
}

impl Default for MarketUi {
    fn default() -> Self {
        Self {
            requested: false,
            loading: false,
            error: None,
            entries: Vec::new(),
            search: None,
            limit: MARKET_PAGE_SIZE,
            pending: HashSet::new(),
        }
    }
}

/// 市场条目关键字过滤：名称 / id / 描述 / 作者 / 标签任一命中（大小写不敏感）。
pub fn filter_market<'a>(entries: &'a [MarketEntryDto], query: &str) -> Vec<&'a MarketEntryDto> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return entries.iter().collect();
    }
    let hit = |value: &str| value.to_lowercase().contains(&query);
    entries
        .iter()
        .filter(|entry| {
            hit(&entry.name)
                || hit(&entry.plugin_id)
                || hit(&entry.description)
                || hit(&entry.author)
                || entry.tags.iter().any(|tag| hit(tag))
        })
        .collect()
}

impl ExtensionsView {
    pub(crate) fn render_plugins(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.ensure_market_loaded(window, cx);
        let search = self.ensure_market_search(window, cx);
        let query = search.read(cx).value().to_string();
        let frame = Frame {
            translator: self.translator.read(cx),
            theme: cx.theme(),
            stale: self.controller.is_stale(),
        };
        let Frame {
            translator,
            theme,
            stale,
        } = frame;

        let installed = self
            .controller
            .plugins()
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                self.render_plugin_card(index, plugin, frame, cx)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let installed_ids = self
            .controller
            .plugins()
            .iter()
            .map(|plugin| plugin.identity.as_str())
            .collect::<HashSet<_>>();

        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(translator.text("pluginsSectionTitle").to_owned()),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(translator.text("pluginDevModeSwitch").to_owned()),
                            )
                            .child(
                                Switch::new("plugin-dev-mode")
                                    .checked(self.plugins.dev_mode)
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        this.plugins.dev_mode = *checked;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(self.render_install_area(frame, cx))
            .when(installed.is_empty(), |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(translator.text("pluginsEmpty").to_owned()),
                )
            })
            .children(installed)
            .child(
                v_flex()
                    .w_full()
                    .pt_4()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .child(translator.text("marketSectionTitle").to_owned()),
                            )
                            .child(
                                Button::new("market-refresh")
                                    .ghost()
                                    .small()
                                    .label(translator.text("marketRefreshTooltip").to_owned())
                                    .loading(self.plugins.market.loading)
                                    .disabled(self.plugins.market.loading || stale)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.load_market(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(translator.text("marketSectionDesc").to_owned()),
                    ),
            )
            .child(self.render_market(&search, &query, &installed_ids, frame, cx))
    }

    fn ensure_market_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(search) = &self.plugins.market.search {
            return search.clone();
        }
        let placeholder = self
            .translator
            .read(cx)
            .text("marketSearchPlaceholder")
            .to_owned();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.plugins.market.limit = MARKET_PAGE_SIZE;
                cx.notify();
            }
        })
        .detach();
        self.plugins.market.search = Some(search.clone());
        search
    }

    fn render_install_area(&self, frame: Frame<'_>, cx: &Context<Self>) -> impl IntoElement {
        let Frame {
            translator,
            theme,
            stale,
        } = frame;
        let dev_dir = self.plugins.dev_dir.clone();
        let dev_dir_empty = dev_dir.is_empty();
        h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .p_3()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(
                Button::new("plugin-install-zip")
                    .outline()
                    .small()
                    .icon(IconName::FolderOpen)
                    .label(translator.text("pluginInstallZipButton").to_owned())
                    .loading(self.plugins.installing_file)
                    .disabled(stale || self.plugins.installing_file)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.pick_plugin_zip(window, cx);
                    })),
            )
            .when(self.plugins.dev_mode, |this| {
                this.child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .text_color(if dev_dir_empty {
                                    theme.muted_foreground
                                } else {
                                    theme.foreground
                                })
                                .child(if dev_dir_empty {
                                    translator.text("pluginInstallDirPlaceholder").to_owned()
                                } else {
                                    dev_dir
                                }),
                        )
                        .child(
                            Button::new("plugin-pick-dev-dir")
                                .outline()
                                .small()
                                .icon(IconName::FolderOpen)
                                .tooltip(translator.text("pluginInstallDirLabel").to_owned())
                                .disabled(self.plugins.installing_dir)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_dev_dir(window, cx);
                                })),
                        )
                        .child(
                            Button::new("plugin-install-dev-dir")
                                .primary()
                                .small()
                                .label(translator.text("pluginInstallDirButton").to_owned())
                                .loading(self.plugins.installing_dir)
                                .disabled(stale || dev_dir_empty || self.plugins.installing_dir)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.install_dev_dir(window, cx);
                                })),
                        ),
                )
            })
    }

    fn render_plugin_card(
        &self,
        index: usize,
        plugin: &PluginDto,
        frame: Frame<'_>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let Frame {
            translator,
            theme,
            stale,
        } = frame;
        let identity = plugin.identity.clone();
        let busy = stale || self.plugins.busy.contains(&plugin.identity);
        let badges = [
            plugin
                .dev_mode
                .then(|| Tag::info().child(translator.text("pluginDevModeBadge").to_owned())),
            (plugin.disabled_reason == "Manual").then(|| {
                Tag::secondary().child(translator.text("pluginDisabledManual").to_owned())
            }),
            (plugin.disabled_reason == "CircuitBreaker").then(|| {
                Tag::danger().child(translator.text("pluginDisabledCircuitBreaker").to_owned())
            }),
        ];
        let detail = PluginDetail::from_plugin(plugin);
        let detail_translator = translator.clone();
        h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .px_3()
            .py_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .flex_wrap()
                            .child(div().text_sm().font_semibold().child(plugin.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("v{}", plugin.version)),
                            )
                            .when(!plugin.homepage.is_empty(), |this| {
                                this.child(
                                    Link::new(("plugin-homepage", index))
                                        .href(plugin.homepage.clone())
                                        .text_xs()
                                        .child(plugin.homepage.clone()),
                                )
                            })
                            .children(badges.into_iter().flatten()),
                    )
                    .when(!plugin.description.is_empty(), |this| {
                        this.child(
                            div()
                                .w_full()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(plugin.description.clone()),
                        )
                    }),
            )
            .child(
                Button::new(("plugin-detail", index))
                    .ghost()
                    .small()
                    .icon(IconName::Info)
                    .tooltip(translator.text("pluginDetailDescription").to_owned())
                    .on_click(move |_, window, cx| {
                        open_plugin_detail(detail.clone(), detail_translator.clone(), window, cx);
                    }),
            )
            .child(
                Switch::new(("plugin-enabled", index))
                    .checked(plugin.enabled)
                    .disabled(busy)
                    .on_click({
                        let identity = identity.clone();
                        cx.listener(move |this, checked: &bool, window, cx| {
                            let future = this
                                .controller
                                .set_plugin_enabled(identity.clone(), *checked);
                            this.run_plugin_op(
                                identity.clone(),
                                PluginOp::SetEnabled,
                                future,
                                window,
                                cx,
                            );
                        })
                    }),
            )
            .when(!plugin.settings.is_empty(), |this| {
                let identity = identity.clone();
                this.child(
                    Button::new(("plugin-settings", index))
                        .ghost()
                        .small()
                        .icon(IconName::Settings2)
                        .tooltip(translator.text("pluginSettingsTooltip").to_owned())
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_plugin_settings(&identity, window, cx);
                        })),
                )
            })
            .child(
                Button::new(("plugin-uninstall", index))
                    .ghost()
                    .small()
                    .icon(IconName::Delete)
                    .tooltip(translator.text("pluginUninstallTooltip").to_owned())
                    .disabled(busy)
                    .on_click({
                        let name = plugin.name.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.confirm_uninstall_plugin(
                                identity.clone(),
                                name.clone(),
                                window,
                                cx,
                            );
                        })
                    }),
            )
    }

    fn render_market(
        &self,
        search: &Entity<InputState>,
        query: &str,
        installed_ids: &HashSet<&str>,
        frame: Frame<'_>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let Frame {
            translator, theme, ..
        } = frame;
        let market = &self.plugins.market;
        let mut root = v_flex().w_full().gap_2();
        if market.loading && market.entries.is_empty() {
            return root.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(translator.text("pluginCommonLoading").to_owned()),
            );
        }
        if let Some(error) = &market.error {
            return root.child(
                div()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(translator.text_with("marketLoadFailed", &[("message", error)])),
            );
        }
        if market.entries.is_empty() {
            return root.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(translator.text("marketEmpty").to_owned()),
            );
        }
        root = root.child(Input::new(search).w_full().cleanable(true));
        let filtered = filter_market(&market.entries, query);
        if filtered.is_empty() {
            return root.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(translator.text("marketSearchNoResult").to_owned()),
            );
        }
        let remaining = filtered.len().saturating_sub(market.limit);
        let cards = filtered
            .iter()
            .take(market.limit)
            .enumerate()
            .map(|(index, entry)| {
                let installed = installed_ids.contains(entry.plugin_id.as_str());
                let pending = market.pending.contains(&entry.plugin_id);
                self.render_market_card(index, entry, installed, pending, frame, cx)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        root.children(cards).when(remaining > 0, |this| {
            this.child(
                Button::new("market-show-more")
                    .ghost()
                    .small()
                    .label(
                        translator
                            .text_with("marketShowMore", &[("count", &remaining.to_string())]),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.plugins.market.limit += MARKET_PAGE_SIZE;
                        cx.notify();
                    })),
            )
        })
    }

    fn render_market_card(
        &self,
        index: usize,
        entry: &MarketEntryDto,
        installed: bool,
        pending: bool,
        frame: Frame<'_>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let Frame {
            translator,
            theme,
            stale,
        } = frame;
        let name = if entry.name.is_empty() {
            entry.plugin_id.clone()
        } else {
            entry.name.clone()
        };
        let yanked = yanked_label(translator, &entry.yanked);
        let detail = PluginDetail::from_market(entry, translator);
        let detail_translator = translator.clone();
        let plugin_id = entry.plugin_id.clone();
        let install_label = if installed {
            translator.text("marketInstalledButton")
        } else if pending {
            translator.text("marketInstallingButton")
        } else {
            translator.text("marketInstallButton")
        }
        .to_owned();
        h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .px_3()
            .py_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .flex_wrap()
                            .child(div().text_sm().font_semibold().child(name))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("v{}", entry.version)),
                            )
                            .when(!entry.author.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(entry.author.clone()),
                                )
                            })
                            .when(!entry.homepage.is_empty(), |this| {
                                this.child(
                                    Link::new(("market-homepage", index))
                                        .href(entry.homepage.clone())
                                        .text_xs()
                                        .child(entry.homepage.clone()),
                                )
                            })
                            .children(yanked.map(|label| Tag::danger().child(label))),
                    )
                    .when(!entry.description.is_empty(), |this| {
                        this.child(
                            div()
                                .w_full()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(entry.description.clone()),
                        )
                    }),
            )
            .child(
                Button::new(("market-detail", index))
                    .ghost()
                    .small()
                    .icon(IconName::Info)
                    .tooltip(translator.text("pluginDetailDescription").to_owned())
                    .on_click(move |_, window, cx| {
                        open_plugin_detail(detail.clone(), detail_translator.clone(), window, cx);
                    }),
            )
            .child(
                Button::new(("market-install", index))
                    .outline()
                    .small()
                    .label(install_label)
                    .loading(pending)
                    .disabled(installed || pending || stale)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.install_market_plugin(plugin_id.clone(), window, cx);
                    })),
            )
    }

    fn ensure_market_loaded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.plugins.market.requested || self.controller.is_stale() {
            return;
        }
        self.load_market(window, cx);
    }

    fn load_market(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let market = &mut self.plugins.market;
        market.requested = true;
        market.loading = true;
        market.error = None;
        let future = self.controller.market_list();
        cx.spawn_in(window, async move |this, cx| {
            let result = future.await;
            let _ = this.update(cx, |this, cx| {
                let market = &mut this.plugins.market;
                market.loading = false;
                match result.and_then(|value| {
                    serde_json::from_value::<Vec<MarketEntryDto>>(value).map_err(|_| {
                        fluxdown_protocol::RpcErrorData::new(
                            fluxdown_protocol::ApplicationErrorCode::ProtocolIncompatible,
                            false,
                        )
                    })
                }) {
                    Ok(entries) => {
                        market.entries = entries;
                        market.error = None;
                        market.limit = MARKET_PAGE_SIZE;
                    }
                    Err(error) => {
                        market.error = Some(error_text(this.translator.read(cx), &error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn pick_plugin_zip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.plugins.installing_file {
            return;
        }
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(SharedString::from(
                self.translator
                    .read(cx)
                    .text("pluginInstallZipButton")
                    .to_owned(),
            )),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.plugins.installing_file = true;
                let future = this
                    .controller
                    .install_plugin_file(path.display().to_string());
                cx.spawn_in(window, async move |this, cx| {
                    let result = future.await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.plugins.installing_file = false;
                        this.finish_plugin_op(PluginOp::Install, result, window, cx);
                        cx.notify();
                    });
                })
                .detach();
                cx.notify();
            });
        })
        .detach();
    }

    fn pick_dev_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                    this.plugins.dev_dir = text;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn install_dev_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.plugins.dev_dir.is_empty() || self.plugins.installing_dir {
            return;
        }
        self.plugins.installing_dir = true;
        let future = self
            .controller
            .install_plugin_dev(self.plugins.dev_dir.clone());
        cx.spawn_in(window, async move |this, cx| {
            let result = future.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.plugins.installing_dir = false;
                if result.is_ok() {
                    this.plugins.dev_dir.clear();
                }
                this.finish_plugin_op(PluginOp::Install, result, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn install_market_plugin(
        &mut self,
        plugin_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.plugins.market.pending.insert(plugin_id.clone()) {
            return;
        }
        let future = self.controller.market_install(plugin_id.clone());
        cx.spawn_in(window, async move |this, cx| {
            let result = future.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.plugins.market.pending.remove(&plugin_id);
                this.finish_plugin_op(PluginOp::Install, result, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn run_plugin_op(
        &mut self,
        identity: String,
        op: PluginOp,
        future: crate::PortFuture<serde_json::Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.plugins.busy.insert(identity.clone()) {
            return;
        }
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = future.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.plugins.busy.remove(&identity);
                this.finish_plugin_op(op, result, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// 写操作结果的全局提示；安装成功且缺少基础组件时追加依赖提醒。
    fn finish_plugin_op(
        &mut self,
        op: PluginOp,
        result: Result<serde_json::Value, fluxdown_protocol::RpcErrorData>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let translator = self.translator.read(cx);
        match (op, result) {
            (PluginOp::Install, Ok(value)) => {
                let message = translator.text("pluginOpInstallSuccess").to_owned();
                let missing = serde_json::from_value::<InstalledPlugin>(value)
                    .map(|installed| installed.missing_components)
                    .unwrap_or_default();
                self.toast_success(message, window, cx);
                if !missing.is_empty() {
                    self.show_missing_components(&missing, window, cx);
                }
            }
            (PluginOp::Uninstall, Ok(_)) => {
                let message = translator.text("pluginOpUninstallSuccess").to_owned();
                self.toast_success(message, window, cx);
            }
            (PluginOp::SetEnabled, Ok(_)) => {}
            (op, Err(error)) => {
                let detail = error_text(translator, &error);
                let key = match op {
                    PluginOp::Install => "pluginOpInstallFailed",
                    PluginOp::Uninstall => "pluginOpUninstallFailed",
                    PluginOp::SetEnabled => "pluginOpEnabledFailed",
                };
                let message = translator.text_with(key, &[("message", &detail)]);
                self.toast_error(message, window, cx);
            }
        }
    }

    fn show_missing_components(
        &self,
        missing: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let translator = self.translator.read(cx);
        let names = missing
            .iter()
            .map(|component| {
                COMPONENT_KINDS
                    .into_iter()
                    .find(|kind| component_wire_name(*kind) == component)
                    .map(|kind| {
                        translator
                            .text(super::managed_components::title_key(kind))
                            .to_owned()
                    })
                    .unwrap_or_else(|| component.clone())
            })
            .collect::<Vec<_>>()
            .join(", ");
        let title = SharedString::from(translator.text("pluginDepsMissingTitle").to_owned());
        let body = SharedString::from(
            translator.text_with("pluginDepsMissingBody", &[("components", &names)]),
        );
        let later = SharedString::from(translator.text("pluginDepsLater").to_owned());
        let go = SharedString::from(translator.text("pluginDepsGoToComponents").to_owned());
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .title(title.clone())
                .description(body.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(go.clone())
                        .cancel_text(later.clone())
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    let _ =
                        view.update(cx, |this, cx| this.show_tab(ExtensionsTab::Components, cx));
                    true
                })
        });
    }

    fn confirm_uninstall_plugin(
        &self,
        identity: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let translator = self.translator.read(cx);
        let title = SharedString::from(translator.text("pluginUninstallTitle").to_owned());
        let body =
            SharedString::from(translator.text_with("pluginUninstallMsg", &[("name", &name)]));
        let ok = SharedString::from(translator.text("pluginUninstallTooltip").to_owned());
        let cancel = SharedString::from(translator.text("cancel").to_owned());
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let identity = identity.clone();
            alert
                .title(title.clone())
                .description(body.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(ok.clone())
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(cancel.clone())
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        let future = this.controller.uninstall_plugin(identity.clone());
                        this.run_plugin_op(
                            identity.clone(),
                            PluginOp::Uninstall,
                            future,
                            window,
                            cx,
                        );
                    });
                    true
                })
        });
    }

    fn open_plugin_settings(&self, identity: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plugin) = self.controller.plugin(identity) else {
            return;
        };
        let translator = self.translator.read(cx);
        let title = SharedString::from(
            translator.text_with("pluginSettingsDialogTitle", &[("name", &plugin.name)]),
        );
        let save = SharedString::from(translator.text("pluginSettingsSaveButton").to_owned());
        let saving = SharedString::from(translator.text("pluginSettingsSaving").to_owned());
        let cancel = SharedString::from(translator.text("cancel").to_owned());
        let translator_entity = self.translator.clone();
        let port = self.controller.port().clone();
        let form =
            cx.new(|cx| PluginSettingsForm::new(translator_entity, port, plugin, window, cx));
        window.open_dialog(cx, move |dialog, _, cx| {
            let is_saving = form.read(cx).is_saving();
            let form_for_content = form.clone();
            let form_for_save = form.clone();
            dialog
                .title(title.clone())
                .w(px(460.))
                .overlay_closable(!is_saving)
                .content(move |content, _, _| content.child(form_for_content.clone()))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("plugin-settings-cancel")
                                .outline()
                                .label(cancel.clone())
                                .disabled(is_saving)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("plugin-settings-save")
                                .primary()
                                .label(if is_saving {
                                    saving.clone()
                                } else {
                                    save.clone()
                                })
                                .loading(is_saving)
                                .disabled(is_saving)
                                .on_click(move |_, window, cx| {
                                    form_for_save.update(cx, |form, cx| form.submit(window, cx));
                                }),
                        ),
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::MarketEntryDto;

    use super::filter_market;

    fn entry(
        id: &str,
        name: &str,
        description: &str,
        author: &str,
        tags: &[&str],
    ) -> MarketEntryDto {
        MarketEntryDto {
            plugin_id: id.to_owned(),
            version: "1.0.0".to_owned(),
            sequence: 1,
            content_hash: String::new(),
            min_app_version: String::new(),
            name: name.to_owned(),
            description: description.to_owned(),
            author: author.to_owned(),
            homepage: String::new(),
            mirrors: Vec::new(),
            publish_time: String::new(),
            yanked: String::new(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            permissions: Vec::new(),
        }
    }

    #[test]
    fn empty_query_keeps_everything() {
        let entries = [
            entry("a", "Alpha", "", "", &[]),
            entry("b", "Beta", "", "", &[]),
        ];
        assert_eq!(filter_market(&entries, "   ").len(), 2);
    }

    #[test]
    fn query_matches_any_field_case_insensitively() {
        let entries = [
            entry("video.dl", "Video", "grabs videos", "Ann", &["media"]),
            entry("other", "Other", "misc", "Bob", &["tools"]),
        ];
        let ids = |query: &str| {
            filter_market(&entries, query)
                .into_iter()
                .map(|entry| entry.plugin_id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("VIDEO"), vec!["video.dl"]);
        assert_eq!(ids("bob"), vec!["other"]);
        assert_eq!(ids("media"), vec!["video.dl"]);
        assert_eq!(ids("grabs"), vec!["video.dl"]);
        assert!(ids("nothing").is_empty());
    }
}
