use fluxdown_ui_components::sidebar_navigation_button;
use fluxdown_ui_theme::active_theme;
use gpui::{
    App, Context, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div, percentage,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, animation::ease_in_out_cubic, h_flex,
    scroll::ScrollableElement as _, v_flex,
};

use crate::{
    model::{
        DownloadCategory, DownloadFilter, DownloadStatusFilter, SidebarSection, SidebarSelection,
        StatusFolderMotion,
    },
    pages::downloads::DownloadView,
};

impl DownloadView {
    fn section_header(
        &self,
        id: &'static str,
        label: SharedString,
        section: SidebarSection,
        open_amount: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        let header_color = tokens.colors.muted_foreground.opacity(0.65);
        let chevron_rotation = percentage(open_amount * 0.25);

        h_flex()
            .id(id)
            .h(px(28.))
            .px(tokens.spacing.sm)
            .items_center()
            .justify_between()
            .cursor_pointer()
            .rounded(tokens.radius.sm)
            .text_size(px(10.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(header_color)
            .hover(|style| {
                style
                    .bg(tokens.colors.muted)
                    .text_color(tokens.colors.muted_foreground)
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if matches!(section, SidebarSection::Queues) {
                    this.retarget_queue_section(cx);
                }
                cx.notify();
            }))
            .child(label)
            .child(
                Icon::new(IconName::ChevronRight)
                    .size(px(12.))
                    .rotate(chevron_rotation),
            )
    }

    fn nav_item(
        &self,
        id: &'static str,
        selection: SidebarSelection,
        label: SharedString,
        icon: IconName,
        trailing: (SharedString, bool),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (count, show_dot) = trailing;
        let tokens = active_theme(cx).tokens();
        let selected = self.selected_item == selection;
        let count_color = if selected {
            tokens.colors.accent_foreground
        } else {
            tokens.colors.muted_foreground.opacity(0.65)
        };
        let dot_color = if selection == SidebarSelection::MainQueue {
            cx.theme().success
        } else {
            tokens.colors.muted_foreground
        };
        let trailing = h_flex()
            .flex_none()
            .items_center()
            .gap(tokens.spacing.xs)
            .when(show_dot, |this| {
                this.child(div().size(px(5.)).rounded_full().bg(dot_color))
            })
            .child(
                div()
                    .min_w(px(12.))
                    .text_right()
                    .text_size(px(11.))
                    .text_color(count_color)
                    .child(count),
            );

        sidebar_navigation_button(
            id,
            label,
            Icon::new(icon).size(px(14.)),
            trailing,
            selected,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_item(selection, cx);
        }))
    }

    fn select_item(&mut self, selection: SidebarSelection, cx: &mut Context<Self>) {
        if self.selected_item == selection {
            return;
        }
        self.selected_item = selection;
        match selection {
            SidebarSelection::Download(filter) => {
                self.table_state.update(cx, |table, cx| {
                    table.delegate_mut().set_filter(filter);
                    table.refresh(cx);
                });
            }
            SidebarSelection::MainQueue => {
                self.table_state.update(cx, |table, cx| {
                    table
                        .delegate_mut()
                        .set_queue_filter(fluxdown_protocol::MAIN_QUEUE_ID);
                    table.refresh(cx);
                });
            }
            SidebarSelection::LaterQueue => {
                self.table_state.update(cx, |table, cx| {
                    table
                        .delegate_mut()
                        .set_queue_filter(fluxdown_protocol::LATER_QUEUE_ID);
                    table.refresh(cx);
                });
            }
        }
        cx.notify();
    }

    fn filter_count(&self, filter: DownloadFilter, cx: &Context<Self>) -> SharedString {
        SharedString::from(
            self.table_state
                .read(cx)
                .delegate()
                .count_matching(filter)
                .to_string(),
        )
    }

    fn queue_count(&self, queue_id: &str, cx: &Context<Self>) -> SharedString {
        SharedString::from(
            self.table_state
                .read(cx)
                .delegate()
                .count_in_queue(queue_id)
                .to_string(),
        )
    }

    fn folder_item(
        &self,
        id: &'static str,
        status: DownloadStatusFilter,
        label: SharedString,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        let filter = DownloadFilter::new(status, DownloadCategory::All);
        let selection = SidebarSelection::Download(filter);
        let selected = self.selected_item == selection;
        let foreground = if selected {
            tokens.colors.accent_foreground
        } else {
            tokens.colors.muted_foreground
        };
        let trailing = h_flex()
            .flex_none()
            .items_center()
            .gap(tokens.spacing.xs)
            .text_color(foreground)
            .child(
                div()
                    .min_w(px(12.))
                    .text_right()
                    .text_size(px(11.))
                    .child(self.filter_count(filter, cx)),
            )
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(12.)),
            );

        sidebar_navigation_button(
            id,
            label,
            Icon::new(if expanded {
                IconName::FolderOpen
            } else {
                IconName::Folder
            })
            .size(px(14.)),
            trailing,
            selected,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.retarget_status_folder(status, cx);
            this.select_item(selection, cx);
            cx.notify();
        }))
    }

    fn category_item(
        &self,
        id: &'static str,
        status: DownloadStatusFilter,
        category: DownloadCategory,
        label: SharedString,
        icon: IconName,
        cx: &mut Context<Self>,
    ) -> Div {
        let filter = DownloadFilter::new(status, category);
        div().w_full().pl(px(18.)).child(self.nav_item(
            id,
            SidebarSelection::Download(filter),
            label,
            icon,
            (self.filter_count(filter, cx), false),
            cx,
        ))
    }

    fn render_categories(&self, status: DownloadStatusFilter, cx: &mut Context<Self>) -> Div {
        let items = match status {
            DownloadStatusFilter::All => [
                ("download-nav-all-video", DownloadCategory::Video),
                ("download-nav-all-audio", DownloadCategory::Audio),
                ("download-nav-all-document", DownloadCategory::Document),
                ("download-nav-all-image", DownloadCategory::Image),
                ("download-nav-all-program", DownloadCategory::Program),
                ("download-nav-all-archive", DownloadCategory::Archive),
                ("download-nav-all-other", DownloadCategory::Other),
            ],
            DownloadStatusFilter::Completed => [
                ("download-nav-completed-video", DownloadCategory::Video),
                ("download-nav-completed-audio", DownloadCategory::Audio),
                (
                    "download-nav-completed-document",
                    DownloadCategory::Document,
                ),
                ("download-nav-completed-image", DownloadCategory::Image),
                ("download-nav-completed-program", DownloadCategory::Program),
                ("download-nav-completed-archive", DownloadCategory::Archive),
                ("download-nav-completed-other", DownloadCategory::Other),
            ],
            DownloadStatusFilter::Incomplete => [
                ("download-nav-incomplete-video", DownloadCategory::Video),
                ("download-nav-incomplete-audio", DownloadCategory::Audio),
                (
                    "download-nav-incomplete-document",
                    DownloadCategory::Document,
                ),
                ("download-nav-incomplete-image", DownloadCategory::Image),
                ("download-nav-incomplete-program", DownloadCategory::Program),
                ("download-nav-incomplete-archive", DownloadCategory::Archive),
                ("download-nav-incomplete-other", DownloadCategory::Other),
            ],
        };

        v_flex()
            .w_full()
            .children(items.into_iter().map(|(id, category)| {
                let (label, icon) = match category {
                    DownloadCategory::Video => (
                        self.strings.category_video.clone(),
                        IconName::GalleryVerticalEnd,
                    ),
                    DownloadCategory::Audio => {
                        (self.strings.category_audio.clone(), IconName::File)
                    }
                    DownloadCategory::Document => {
                        (self.strings.category_document.clone(), IconName::File)
                    }
                    DownloadCategory::Image => (
                        self.strings.category_image.clone(),
                        IconName::GalleryVerticalEnd,
                    ),
                    DownloadCategory::Program => {
                        (self.strings.category_program.clone(), IconName::HardDrive)
                    }
                    DownloadCategory::Archive => {
                        (self.strings.category_archive.clone(), IconName::Inbox)
                    }
                    DownloadCategory::Other | DownloadCategory::All => {
                        (self.strings.category_other.clone(), IconName::File)
                    }
                };
                self.category_item(id, status, category, label, icon, cx)
            }))
    }

    fn retarget_status_folder(&mut self, status: DownloadStatusFilter, cx: &App) {
        let next = DownloadStatusFilter::exclusive_toggle(self.expanded_status, status);
        if next == self.expanded_status {
            return;
        }
        if cx.reduce_motion() {
            self.folder_motion = StatusFolderMotion::settled(next);
            self.folder_motion_started_at = None;
        } else {
            self.folder_motion = self.folder_motion.retarget(
                ease_in_out_cubic(self.folder_motion_linear_progress()),
                next,
            );
            self.folder_motion_started_at = Some(std::time::Instant::now());
        }
        self.expanded_status = next;
    }

    fn sidebar_motion_linear_progress(started_at: Option<std::time::Instant>) -> f32 {
        let Some(started_at) = started_at else {
            return 1.;
        };
        (started_at.elapsed().as_secs_f32()
            / crate::pages::downloads::SIDEBAR_MOTION_DURATION.as_secs_f32())
        .clamp(0., 1.)
    }

    fn folder_motion_linear_progress(&self) -> f32 {
        Self::sidebar_motion_linear_progress(self.folder_motion_started_at)
    }

    fn retarget_queue_section(&mut self, cx: &App) {
        let target = if self.queues_expanded { 1. } else { 0. };
        let progress = ease_in_out_cubic(Self::sidebar_motion_linear_progress(
            self.queue_motion_started_at,
        ));
        let current = self.queue_motion_from + (target - self.queue_motion_from) * progress;
        let next = !self.queues_expanded;
        if cx.reduce_motion() {
            self.queue_motion_from = if next { 1. } else { 0. };
            self.queue_motion_started_at = None;
        } else {
            self.queue_motion_from = current;
            self.queue_motion_started_at = Some(std::time::Instant::now());
        }
        self.queues_expanded = next;
    }

    fn queue_open_amount(&self, window: &mut Window, cx: &App) -> f32 {
        let target = if self.queues_expanded { 1. } else { 0. };
        if cx.reduce_motion() {
            return target;
        }
        let linear = Self::sidebar_motion_linear_progress(self.queue_motion_started_at);
        if linear < 1. {
            window.request_animation_frame();
        }
        self.queue_motion_from + (target - self.queue_motion_from) * ease_in_out_cubic(linear)
    }

    fn folder_open_amount(
        &self,
        status: DownloadStatusFilter,
        window: &mut Window,
        cx: &App,
    ) -> f32 {
        if cx.reduce_motion() {
            return if self.expanded_status == Some(status) {
                1.
            } else {
                0.
            };
        }
        let linear = self.folder_motion_linear_progress();
        if linear < 1. {
            window.request_animation_frame();
        }
        self.folder_motion.amount(status, ease_in_out_cubic(linear))
    }

    fn render_filter_branch(
        &self,
        id: &'static str,
        status: DownloadStatusFilter,
        label: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let expanded = self.expanded_status == Some(status);
        let open_amount = self.folder_open_amount(status, window, cx);
        v_flex()
            .w_full()
            .child(self.folder_item(id, status, label, expanded, cx))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .h(px(32. * 7. * open_amount))
                    .child(self.render_categories(status, cx)),
            )
    }

    fn render_download_tree(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .mx(tokens.spacing.sm)
            .mt(tokens.spacing.sm)
            .px(tokens.spacing.xs)
            .py(tokens.spacing.xs)
            .border_1()
            .border_color(tokens.colors.border)
            .rounded(tokens.radius.md)
            .child(self.render_filter_branch(
                "download-folder-all",
                DownloadStatusFilter::All,
                self.strings.status_all.clone(),
                window,
                cx,
            ))
            .child(self.render_filter_branch(
                "download-folder-completed",
                DownloadStatusFilter::Completed,
                self.strings.status_completed.clone(),
                window,
                cx,
            ))
            .child(self.render_filter_branch(
                "download-folder-incomplete",
                DownloadStatusFilter::Incomplete,
                self.strings.status_incomplete.clone(),
                window,
                cx,
            ))
    }

    fn render_queue_section(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let open_amount = self.queue_open_amount(window, cx);
        v_flex()
            .mx(tokens.spacing.sm)
            .mt(tokens.spacing.sm)
            .px(tokens.spacing.xs)
            .py(tokens.spacing.xs)
            .border_1()
            .border_color(tokens.colors.border)
            .rounded(tokens.radius.md)
            .child(self.section_header(
                "download-queue-toggle",
                self.strings.sidebar_queues.clone(),
                SidebarSection::Queues,
                open_amount,
                cx,
            ))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .h(px(32. * 2. * open_amount))
                    .child(
                        v_flex()
                            .w_full()
                            .opacity(open_amount)
                            .child(self.nav_item(
                                "download-nav-main-queue",
                                SidebarSelection::MainQueue,
                                self.strings.main_queue.clone(),
                                IconName::GalleryVerticalEnd,
                                (self.queue_count(fluxdown_protocol::MAIN_QUEUE_ID, cx), true),
                                cx,
                            ))
                            .child(self.nav_item(
                                "download-nav-later-queue",
                                SidebarSelection::LaterQueue,
                                self.strings.later_queue.clone(),
                                IconName::Pause,
                                (
                                    self.queue_count(fluxdown_protocol::LATER_QUEUE_ID, cx),
                                    true,
                                ),
                                cx,
                            )),
                    ),
            )
    }

    pub(crate) fn render_sidebar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let surface = active_theme(cx).tokens().colors.surface;
        v_flex()
            .size_full()
            .min_w_0()
            .overflow_y_scrollbar()
            .bg(surface)
            .child(self.render_download_tree(window, cx))
            .child(self.render_queue_section(window, cx))
    }
}
