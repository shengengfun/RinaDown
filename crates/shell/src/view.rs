use std::rc::Rc;

use fluxdown_ui_components::activity_button as activity_bar_button;
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyElement, AnyView, App, Context, Div, Entity, Img, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, div, img, px,
};
use gpui_component::{
    Icon, Sizable as _, TITLE_BAR_HEIGHT, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::DropdownMenu as _,
    tooltip::Tooltip,
    v_flex,
};

use crate::{assets::APP_LOGO_PATH, strings::ShellStrings};

/// shell 路由的稳定标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteId(&'static str);

impl RouteId {
    /// 创建稳定路由标识。
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }
}

/// 由应用装配并注入 shell 的路由元数据与页面内容。
pub struct ShellRoute {
    id: RouteId,
    button_id: &'static str,
    tooltip_id: &'static str,
    label_key: &'static str,
    icon: Icon,
    view: AnyView,
}

impl ShellRoute {
    /// 创建一条 shell 路由。
    pub fn new(
        id: RouteId,
        button_id: &'static str,
        tooltip_id: &'static str,
        label_key: &'static str,
        icon: Icon,
        view: AnyView,
    ) -> Self {
        Self {
            id,
            button_id,
            tooltip_id,
            label_key,
            icon,
            view,
        }
    }
}

type ShellActionHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// 由应用装配并注入 shell 的窗口级活动栏动作。
pub struct ShellAction {
    button_id: &'static str,
    tooltip_id: &'static str,
    label_key: &'static str,
    icon: Icon,
    handler: ShellActionHandler,
}

impl ShellAction {
    /// 创建不切换主内容路由的活动栏动作。
    pub fn new(
        button_id: &'static str,
        tooltip_id: &'static str,
        label_key: &'static str,
        icon: Icon,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            button_id,
            tooltip_id,
            label_key,
            icon,
            handler: Rc::new(handler),
        }
    }
}

/// 使用 FluxDown 自定义标题栏承载任意能力页面的辅助窗口。
pub struct AuxiliaryWindowView {
    _translator: Entity<Translator>,
    title: SharedString,
    content: AnyView,
}

impl AuxiliaryWindowView {
    /// 创建跟随共享语言状态更新标题的辅助窗口 chrome。
    pub fn new(
        translator: Entity<Translator>,
        title_key: &'static str,
        content: AnyView,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = SharedString::from(translator.read(cx).text(title_key).to_owned());
        cx.observe(&translator, move |this, translator, cx| {
            this.title = SharedString::from(translator.read(cx).text(title_key).to_owned());
            cx.notify();
        })
        .detach();
        Self {
            _translator: translator,
            title,
            content,
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        let spacing = tokens.spacing;
        let typography = tokens.typography.clone();
        let title_bar = TitleBar::new();
        #[cfg(not(target_os = "macos"))]
        let title_bar = title_bar.pl(spacing.sm);

        title_bar
            .h(TITLE_BAR_HEIGHT)
            .bg(tokens.colors.surface)
            .border_color(tokens.colors.border)
            .child(
                h_flex()
                    .size_full()
                    .min_w_0()
                    .items_center()
                    .pl(spacing.sm)
                    .pr(spacing.md)
                    .child(
                        div()
                            .min_w_0()
                            .text_size(typography.sm.size)
                            .font_weight(typography.sm.weight)
                            .child(self.title.clone()),
                    ),
            )
    }
}

impl Render for AuxiliaryWindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = active_theme(cx).tokens().colors;
        v_flex()
            .size_full()
            .relative()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(self.render_title_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.content.clone()),
            )
            .children(gpui_component::Root::render_sheet_layer(window, cx))
            .children(gpui_component::Root::render_dialog_layer(window, cx))
            .children(gpui_component::Root::render_notification_layer(window, cx))
    }
}

/// GPUI 窗口外壳：只负责窗口 chrome、活动栏、路由与内容槽位。
pub struct ShellView {
    translator: Entity<Translator>,
    strings: ShellStrings,
    active_route: Option<RouteId>,
    routes: Vec<ShellRoute>,
    actions: Vec<ShellAction>,
}

impl ShellView {
    /// 创建 shell，并使用传入顺序的首条路由作为初始页面。
    pub fn new(
        translator: Entity<Translator>,
        routes: Vec<ShellRoute>,
        actions: Vec<ShellAction>,
        cx: &mut Context<Self>,
    ) -> Self {
        let strings = ShellStrings::from_translator(translator.read(cx));
        let active_route = routes.first().map(|route| route.id);
        cx.observe(&translator, |this, translator, cx| {
            this.strings = ShellStrings::from_translator(translator.read(cx));
            cx.notify();
        })
        .detach();
        Self {
            translator,
            strings,
            active_route,
            routes,
            actions,
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        let colors = tokens.colors;
        let spacing = tokens.spacing;
        let typography = tokens.typography.clone();
        let menu_items = [
            ("title-menu-file", self.strings.menu_file.clone()),
            ("title-menu-tasks", self.strings.menu_tasks.clone()),
            ("title-menu-tools", self.strings.menu_tools.clone()),
            ("title-menu-help", self.strings.menu_help.clone()),
        ];
        let menu_placeholder = self.strings.menu_items_pending.clone();
        let logo = img(APP_LOGO_PATH).size(px(16.));
        let title_bar = TitleBar::new();
        #[cfg(not(target_os = "macos"))]
        let title_bar = title_bar.pl(spacing.sm);
        #[cfg(target_os = "macos")]
        let (leading_logo, trailing_logo): (Option<Img>, Option<Img>) = (None, Some(logo));
        #[cfg(not(target_os = "macos"))]
        let (leading_logo, trailing_logo): (Option<Img>, Option<Img>) = (Some(logo), None);

        title_bar
            .h(TITLE_BAR_HEIGHT)
            .bg(colors.surface)
            .border_color(colors.border)
            .child(
                h_flex()
                    .size_full()
                    .min_w_0()
                    .items_center()
                    .gap(spacing.sm)
                    .pl(spacing.xxs)
                    .pr(spacing.md)
                    .children(leading_logo)
                    .child(
                        h_flex()
                            .h_full()
                            .items_center()
                            .text_size(typography.sm.size)
                            .font_weight(typography.sm.weight)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .children(menu_items.into_iter().map(|(id, label)| {
                                let placeholder = menu_placeholder.clone();
                                Button::new(id)
                                    .label(label)
                                    .xsmall()
                                    .text()
                                    .compact()
                                    .h_full()
                                    .px(spacing.sm)
                                    .cursor_pointer()
                                    .dropdown_menu(move |menu, _, _| {
                                        menu.min_w(140.).label(placeholder.clone())
                                    })
                            })),
                    )
                    .child(div().flex_1())
                    .children(trailing_logo),
            )
    }

    fn route_button(&self, route: &ShellRoute, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.active_route == Some(route.id);
        let label = SharedString::from(self.translator.read(cx).text(route.label_key).to_owned());
        let tooltip_label = label.clone();
        let route_id = route.id;

        div()
            .id(route.tooltip_id)
            .w(px(38.))
            .h(px(48.))
            .flex()
            .items_center()
            .justify_center()
            .tooltip(move |window, cx| Tooltip::new(tooltip_label.clone()).build(window, cx))
            .child(
                activity_bar_button(
                    route.button_id,
                    label,
                    route.icon.clone().size(px(21.)),
                    selected,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.active_route != Some(route_id) {
                        this.active_route = Some(route_id);
                        cx.notify();
                    }
                })),
            )
            .into_any_element()
    }

    fn route_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().children(self.routes.iter().map(|route| self.route_button(route, cx)))
    }

    fn action_button(&self, action: &ShellAction, cx: &mut Context<Self>) -> AnyElement {
        let label = SharedString::from(self.translator.read(cx).text(action.label_key).to_owned());
        let tooltip_label = label.clone();
        let handler = Rc::clone(&action.handler);

        div()
            .id(action.tooltip_id)
            .w(px(38.))
            .h(px(48.))
            .flex()
            .items_center()
            .justify_center()
            .tooltip(move |window, cx| Tooltip::new(tooltip_label.clone()).build(window, cx))
            .child(
                activity_bar_button(
                    action.button_id,
                    label,
                    action.icon.clone().size(px(21.)),
                    false,
                    cx,
                )
                .on_click(move |_, window, cx| handler(window, cx)),
            )
            .into_any_element()
    }

    fn action_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().children(
            self.actions
                .iter()
                .map(|action| self.action_button(action, cx)),
        )
    }

    fn render_activity_bar(&self, cx: &mut Context<Self>) -> Div {
        let colors = active_theme(cx).tokens().colors;
        v_flex()
            .h_full()
            .w(px(38.))
            .flex_none()
            .justify_between()
            .bg(colors.surface)
            .border_r_1()
            .border_color(colors.border)
            .child(self.route_buttons(cx))
            .child(self.action_buttons(cx))
    }

    fn active_content(&self) -> AnyElement {
        self.active_route
            .and_then(|active| self.routes.iter().find(|route| route.id == active))
            .map_or_else(
                || div().into_any_element(),
                |route| route.view.clone().into_any_element(),
            )
    }
}

impl Render for ShellView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = active_theme(cx).tokens().colors;
        v_flex()
            .size_full()
            .relative()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.render_activity_bar(cx))
                    .child(
                        div()
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(self.active_content()),
                    ),
            )
            .children(gpui_component::Root::render_sheet_layer(window, cx))
            .children(gpui_component::Root::render_dialog_layer(window, cx))
            .children(gpui_component::Root::render_notification_layer(window, cx))
    }
}
