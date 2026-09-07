use std::{cmp::Ordering, collections::HashSet};

use fluxdown_ui_components::toolbar_action_button;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, InteractiveElement as _,
    IntoElement, Modifiers, MouseButton, ParentElement, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
    relative,
};
use gpui_component::{
    ActiveTheme as _, FocusableExt as _, Icon, IconName, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    popover::Popover,
    spinner::Spinner,
    table::{Column, ColumnSort, DataTable, TableDelegate, TableState},
    tooltip::Tooltip,
    v_flex,
};

use crate::{
    controller::DownloadsCommand,
    model::{DownloadFilter, DownloadTaskView, TaskKind, TaskSource, TaskState, format_bytes},
    pages::downloads::{DownloadView, SelectAllTasks, TASK_ROW_HEIGHT},
    strings::DownloadStrings,
};

const SELECTION_COLUMN_WIDTH: f32 = 36.;
const TABLE_HEADER_CROP: f32 = 2.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadColumnKind {
    FileName,
    Size,
    Status,
    Speed,
    Eta,
    Created,
}

impl DownloadColumnKind {
    const ALL: [Self; 6] = [
        Self::FileName,
        Self::Size,
        Self::Status,
        Self::Speed,
        Self::Eta,
        Self::Created,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::FileName => "file_name",
            Self::Size => "size",
            Self::Status => "status",
            Self::Speed => "speed",
            Self::Eta => "eta",
            Self::Created => "created",
        }
    }

    fn default_width(self) -> f32 {
        match self {
            Self::FileName => 240.,
            Self::Size => 90.,
            Self::Status => 150.,
            Self::Speed => 100.,
            Self::Eta => 100.,
            Self::Created => 110.,
        }
    }

    fn min_width(self) -> f32 {
        match self {
            Self::FileName => 160.,
            Self::Size => 72.,
            Self::Status => 118.,
            Self::Speed => 82.,
            Self::Eta => 82.,
            Self::Created => 90.,
        }
    }

    fn label(self, strings: &DownloadStrings) -> SharedString {
        match self {
            Self::FileName => strings.col_file_name.clone(),
            Self::Size => strings.col_size.clone(),
            Self::Status => strings.col_status.clone(),
            Self::Speed => strings.col_speed.clone(),
            Self::Eta => strings.col_eta.clone(),
            Self::Created => strings.col_created.clone(),
        }
    }
}

#[derive(Clone)]
struct DownloadColumn {
    kind: DownloadColumnKind,
    width: f32,
    visible: bool,
}

impl DownloadColumn {
    fn new(kind: DownloadColumnKind) -> Self {
        Self {
            kind,
            width: kind.default_width(),
            visible: true,
        }
    }
}

#[derive(Clone)]
struct DraggedColumnMenuItem {
    kind: DownloadColumnKind,
    label: SharedString,
}

impl Render for DraggedColumnMenuItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens();
        h_flex()
            .px(tokens.spacing.md)
            .py(tokens.spacing.xs)
            .gap(tokens.spacing.sm)
            .rounded(tokens.radius.sm)
            .border_1()
            .border_color(tokens.colors.border)
            .bg(tokens.colors.surface)
            .shadow_sm()
            .text_color(tokens.colors.surface_foreground)
            .child(Icon::new(IconName::Menu).size(px(13.)))
            .child(self.label.clone())
    }
}

pub(crate) struct DownloadTableDelegate {
    strings: DownloadStrings,
    columns: Vec<DownloadColumn>,
    source_items: Vec<DownloadTaskView>,
    items: Vec<DownloadTaskView>,
    selected_tasks: HashSet<String>,
    selection_anchor: Option<String>,
    filter: DownloadFilter,
    queue_id: Option<String>,
}

impl DownloadTableDelegate {
    pub(crate) fn new(strings: DownloadStrings, items: Vec<DownloadTaskView>) -> Self {
        Self {
            strings,
            columns: DownloadColumnKind::ALL
                .into_iter()
                .map(DownloadColumn::new)
                .collect(),
            source_items: items.clone(),
            items,
            selected_tasks: HashSet::new(),
            selection_anchor: None,
            filter: DownloadFilter::ALL,
            queue_id: None,
        }
    }

    pub(crate) fn set_strings(&mut self, strings: DownloadStrings) {
        self.strings = strings;
    }

    pub(crate) fn set_items(&mut self, items: Vec<DownloadTaskView>) {
        self.source_items = items;
        self.apply_filters();
    }

    pub(crate) fn set_filter(&mut self, filter: DownloadFilter) {
        self.filter = filter;
        self.queue_id = None;
        self.apply_filters();
    }

    pub(crate) fn set_queue_filter(&mut self, queue_id: &str) {
        self.filter = DownloadFilter::ALL;
        self.queue_id = Some(queue_id.to_owned());
        self.apply_filters();
    }

    fn apply_filters(&mut self) {
        self.items = self
            .source_items
            .iter()
            .filter(|task| self.filter.matches(task))
            .filter(|task| {
                self.queue_id
                    .as_ref()
                    .is_none_or(|queue_id| task.queue_id == *queue_id)
            })
            .cloned()
            .collect();
        self.selected_tasks
            .retain(|task_id| self.items.iter().any(|task| &task.id == task_id));
        self.selection_anchor = self
            .selection_anchor
            .take()
            .filter(|task_id| self.selected_tasks.contains(task_id));
    }

    pub(crate) fn count_matching(&self, filter: DownloadFilter) -> usize {
        self.source_items
            .iter()
            .filter(|task| filter.matches(task))
            .count()
    }

    pub(crate) fn count_in_queue(&self, queue_id: &str) -> usize {
        self.source_items
            .iter()
            .filter(|task| task.source == TaskSource::Local && task.queue_id == queue_id)
            .count()
    }

    fn visible_columns_count(&self) -> usize {
        self.columns.iter().filter(|column| column.visible).count()
    }

    fn visible_column(&self, col_ix: usize) -> Option<&DownloadColumn> {
        if col_ix == 0 {
            return None;
        }
        self.columns
            .iter()
            .filter(|column| column.visible)
            .nth(col_ix - 1)
    }

    fn move_visible_column(&mut self, from_ix: usize, to_ix: usize) {
        if from_ix == 0 || to_ix == 0 {
            return;
        }
        let visible_positions: Vec<_> = self
            .columns
            .iter()
            .enumerate()
            .filter_map(|(ix, column)| column.visible.then_some(ix))
            .collect();
        let Some(&from_position) = visible_positions.get(from_ix - 1) else {
            return;
        };
        let Some(&to_position) = visible_positions.get(to_ix - 1) else {
            return;
        };
        let column = self.columns.remove(from_position);
        self.columns.insert(to_position, column);
    }

    fn move_column_kind(&mut self, from: DownloadColumnKind, to: DownloadColumnKind) {
        let Some(from_ix) = self.columns.iter().position(|column| column.kind == from) else {
            return;
        };
        let Some(to_ix) = self.columns.iter().position(|column| column.kind == to) else {
            return;
        };
        if from_ix == to_ix {
            return;
        }
        let column = self.columns.remove(from_ix);
        self.columns.insert(to_ix, column);
    }

    fn set_column_visible(&mut self, kind: DownloadColumnKind, visible: bool) -> bool {
        if !visible && self.visible_columns_count() <= 1 {
            return false;
        }
        let Some(column) = self.columns.iter_mut().find(|column| column.kind == kind) else {
            return false;
        };
        if column.visible == visible {
            return false;
        }
        column.visible = visible;
        true
    }

    fn reset_columns(&mut self) {
        self.columns = DownloadColumnKind::ALL
            .into_iter()
            .map(DownloadColumn::new)
            .collect();
    }

    fn select_all_tasks(&mut self) {
        self.selected_tasks.clear();
        self.selected_tasks
            .extend(self.items.iter().map(|task| task.id.clone()));
        self.selection_anchor = None;
    }

    fn select_task(&mut self, task_id: String, modifiers: Modifiers) {
        if modifiers.shift
            && let Some(anchor) = self.selection_anchor.as_ref()
            && let Some(anchor_index) = self.items.iter().position(|task| &task.id == anchor)
            && let Some(task_index) = self.items.iter().position(|task| task.id == task_id)
        {
            let (start, end) = if anchor_index <= task_index {
                (anchor_index, task_index)
            } else {
                (task_index, anchor_index)
            };
            self.selected_tasks.clear();
            self.selected_tasks
                .extend(self.items[start..=end].iter().map(|task| task.id.clone()));
            self.selection_anchor = Some(task_id);
            return;
        }

        if modifiers.secondary() {
            if !self.selected_tasks.remove(&task_id) {
                self.selected_tasks.insert(task_id.clone());
            }
        } else {
            self.selected_tasks.clear();
            self.selected_tasks.insert(task_id.clone());
        }
        self.selection_anchor = Some(task_id);
    }

    pub(crate) fn task_id_at(&self, row_ix: usize) -> Option<String> {
        self.items.get(row_ix).map(|task| task.id.clone())
    }

    pub(crate) fn select_task_for_context_menu(&mut self, task_id: String) {
        if !self.selected_tasks.contains(&task_id) {
            self.selected_tasks.clear();
            self.selected_tasks.insert(task_id.clone());
        }
        self.selection_anchor = Some(task_id);
    }

    fn selected_task_ids(&self) -> Vec<String> {
        self.selected_tasks.iter().cloned().collect()
    }

    fn compare_tasks(
        kind: DownloadColumnKind,
        left: &DownloadTaskView,
        right: &DownloadTaskView,
    ) -> Ordering {
        let ordering = match kind {
            DownloadColumnKind::FileName => left.name.cmp(&right.name),
            DownloadColumnKind::Size => left.size_bytes.cmp(&right.size_bytes),
            DownloadColumnKind::Status => left.state.cmp(&right.state),
            DownloadColumnKind::Speed => left
                .speed_bytes_per_second
                .cmp(&right.speed_bytes_per_second),
            DownloadColumnKind::Eta => left.eta_seconds.cmp(&right.eta_seconds),
            DownloadColumnKind::Created => left.created_order.cmp(&right.created_order),
        };
        ordering
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.id.cmp(&right.id))
    }

    fn task_visuals(
        &self,
        task: &DownloadTaskView,
        cx: &App,
    ) -> (SharedString, gpui::Hsla, IconName, gpui::Hsla, SharedString) {
        let tokens = active_theme(cx).tokens();
        let (status, status_color) = match task.state {
            TaskState::Completed => (self.strings.status_completed.clone(), cx.theme().success),
            TaskState::Paused => (self.strings.status_paused.clone(), cx.theme().warning),
            TaskState::Failed => (self.strings.status_incomplete.clone(), cx.theme().danger),
            TaskState::Pending | TaskState::Downloading => (
                self.strings.status_incomplete.clone(),
                tokens.colors.muted_foreground,
            ),
        };
        let (file_icon, file_icon_color, category) = match task.kind {
            TaskKind::Application => (
                IconName::HardDrive,
                tokens.colors.muted_foreground,
                self.strings.category_program.clone(),
            ),
            TaskKind::Mobile => (
                IconName::MemoryStick,
                tokens.colors.muted_foreground,
                self.strings.category_program.clone(),
            ),
            TaskKind::DiskImage | TaskKind::Archive => (
                IconName::Inbox,
                cx.theme().warning,
                self.strings.category_archive.clone(),
            ),
            TaskKind::Video => (
                IconName::Play,
                tokens.colors.muted_foreground,
                self.strings.category_video.clone(),
            ),
            TaskKind::Audio => (
                IconName::File,
                tokens.colors.muted_foreground,
                self.strings.category_audio.clone(),
            ),
            TaskKind::Document => (
                IconName::File,
                tokens.colors.muted_foreground,
                self.strings.category_document.clone(),
            ),
            TaskKind::Image => (
                IconName::File,
                tokens.colors.muted_foreground,
                self.strings.category_image.clone(),
            ),
            TaskKind::Other => (
                IconName::File,
                tokens.colors.muted_foreground,
                self.strings.category_other.clone(),
            ),
        };
        (status, status_color, file_icon, file_icon_color, category)
    }

    fn render_file_cell(&self, task: &DownloadTaskView, cx: &App) -> AnyElement {
        let tokens = active_theme(cx).tokens();
        let (_, _, file_icon, file_icon_color, category) = self.task_visuals(task, cx);
        let name = if task.metadata_pending {
            self.strings.metadata_loading.clone()
        } else {
            SharedString::from(task.name.clone())
        };
        let icon = if task.metadata_pending {
            Spinner::new()
                .with_size(px(14.))
                .icon(IconName::LoaderCircle)
                .color(tokens.colors.muted_foreground)
                .into_any_element()
        } else {
            Icon::new(file_icon)
                .size(px(14.))
                .text_color(file_icon_color)
                .into_any_element()
        };
        h_flex()
            .size_full()
            .min_w_0()
            .items_center()
            .gap(tokens.spacing.sm)
            .child(div().flex_none().child(icon))
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(relative(1.))
                            .font_weight(tokens.typography.sm.weight)
                            .child(name),
                    )
                    .when(!task.metadata_pending, |this| {
                        this.child(
                            div()
                                .text_size(px(8.))
                                .line_height(relative(1.))
                                .text_color(tokens.colors.muted_foreground)
                                .child(category),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_status_cell(&self, task: &DownloadTaskView, cx: &App) -> AnyElement {
        let tokens = active_theme(cx).tokens();
        let (status, status_color, _, _, _) = self.task_visuals(task, cx);
        v_flex()
            .size_full()
            .justify_center()
            .gap(px(2.))
            .child(
                h_flex()
                    .gap(tokens.spacing.xs)
                    .text_size(tokens.typography.xs.size)
                    .line_height(relative(1.))
                    .font_weight(tokens.typography.xs.weight)
                    .child(task.progress_label.clone())
                    .child(status),
            )
            .child(
                div()
                    .relative()
                    .h(px(4.))
                    .w_full()
                    .max_w(px(110.))
                    .overflow_hidden()
                    .rounded_full()
                    .bg(tokens.colors.muted)
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .w(relative(task.progress))
                            .bg(status_color),
                    ),
            )
            .into_any_element()
    }
}

impl TableDelegate for DownloadTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.visible_columns_count() + 1
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.items.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        if col_ix == 0 {
            return Column::new("selection", "")
                .width(px(SELECTION_COLUMN_WIDTH))
                .min_width(px(SELECTION_COLUMN_WIDTH))
                .max_width(px(SELECTION_COLUMN_WIDTH))
                .fixed_left()
                .resizable(false)
                .movable(false)
                .selectable(false)
                .p_0();
        }

        let Some(column) = self.visible_column(col_ix) else {
            return Column::new("missing", "").resizable(false).movable(false);
        };
        Column::new(column.kind.key(), column.kind.label(&self.strings))
            .width(px(column.width))
            .min_width(px(column.kind.min_width()))
            .max_width(px(480.))
            .sortable()
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if col_ix == 0 {
            let all_tasks_selected = !self.items.is_empty()
                && self
                    .items
                    .iter()
                    .all(|task| self.selected_tasks.contains(&task.id));
            return h_flex()
                .size_full()
                .justify_center()
                .relative()
                .left(px(4.0))
                .top(px(1.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Checkbox::new("select-all-download-tasks")
                        .with_size(Size::XSmall)
                        .checked(all_tasks_selected)
                        .focus_ring(false)
                        .on_click(cx.listener(|table, checked: &bool, _, cx| {
                            let delegate = table.delegate_mut();
                            if *checked {
                                delegate
                                    .selected_tasks
                                    .extend(delegate.items.iter().map(|task| task.id.clone()));
                            } else {
                                delegate.selected_tasks.clear();
                            }
                            delegate.selection_anchor = None;
                            cx.notify();
                        })),
                );
        }

        let tokens = active_theme(cx).tokens();
        let label = self
            .visible_column(col_ix)
            .map(|column| column.kind.label(&self.strings))
            .unwrap_or_default();
        h_flex()
            .size_full()
            .items_center()
            .relative()
            .top(px(1.))
            .text_size(tokens.typography.sm.size)
            .font_weight(tokens.typography.sm.weight)
            .child(label)
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let Some(task) = self.items.get(row_ix).cloned() else {
            return div().id(("download-task-row", row_ix));
        };
        let task_id = task.id.clone();
        let selected = self.selected_tasks.contains(&task_id);
        let selected_background = active_theme(cx).tokens().colors.accent;

        div()
            .id(format!("download-task-row:{task_id}"))
            .relative()
            .when(selected, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(active_theme(cx).tokens().radius.sm)
                        .bg(selected_background),
                )
            })
            .on_click(cx.listener(move |table, event: &ClickEvent, _, cx| {
                table
                    .delegate_mut()
                    .select_task(task_id.clone(), event.modifiers());
                cx.notify();
            }))
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(task) = self.items.get(row_ix).cloned() else {
            return div().into_any_element();
        };
        let tokens = active_theme(cx).tokens();

        if col_ix == 0 {
            let task_id = task.id.clone();
            let selected = self.selected_tasks.contains(&task_id);
            return h_flex()
                .size_full()
                .justify_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Checkbox::new(format!("download-task-multi-select:{task_id}"))
                        .with_size(Size::XSmall)
                        .checked(selected)
                        .focus_ring(false)
                        .on_click(cx.listener(move |table, checked: &bool, _, cx| {
                            let delegate = table.delegate_mut();
                            if *checked {
                                delegate.selected_tasks.insert(task_id.clone());
                            } else {
                                delegate.selected_tasks.remove(&task_id);
                            }
                            delegate.selection_anchor = Some(task_id.clone());
                            cx.notify();
                        })),
                )
                .into_any_element();
        }

        let Some(kind) = self.visible_column(col_ix).map(|column| column.kind) else {
            return div().into_any_element();
        };
        match kind {
            DownloadColumnKind::FileName => self.render_file_cell(&task, cx),
            DownloadColumnKind::Size => h_flex()
                .size_full()
                .items_center()
                .text_size(tokens.typography.xs.size)
                .text_color(tokens.colors.muted_foreground)
                .child(task.size.clone())
                .into_any_element(),
            DownloadColumnKind::Status => self.render_status_cell(&task, cx),
            DownloadColumnKind::Speed => {
                let speed = task
                    .speed_bytes_per_second
                    .filter(|speed| *speed > 0)
                    .map(|speed| format!("{}/s", format_bytes(speed)))
                    .unwrap_or_else(|| "—".to_owned());
                h_flex()
                    .size_full()
                    .items_center()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(speed)
                    .into_any_element()
            }
            DownloadColumnKind::Eta => {
                let eta = task
                    .eta_seconds
                    .filter(|seconds| *seconds <= 86_400)
                    .map_or_else(
                        || SharedString::from("—"),
                        |seconds| self.strings.format_eta(seconds),
                    );
                h_flex()
                    .size_full()
                    .items_center()
                    .text_size(tokens.typography.xs.size)
                    .text_color(tokens.colors.muted_foreground)
                    .child(eta)
                    .into_any_element()
            }
            DownloadColumnKind::Created => h_flex()
                .size_full()
                .items_center()
                .text_size(tokens.typography.xs.size)
                .text_color(tokens.colors.muted_foreground)
                .child(self.strings.today.clone())
                .into_any_element(),
        }
    }

    fn move_column(
        &mut self,
        col_ix: usize,
        to_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        self.move_visible_column(col_ix, to_ix);
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        let Some(kind) = self.visible_column(col_ix).map(|column| column.kind) else {
            return;
        };
        match sort {
            ColumnSort::Default => self.items.sort_by(|left, right| left.id.cmp(&right.id)),
            ColumnSort::Ascending => self
                .items
                .sort_by(|left, right| Self::compare_tasks(kind, left, right)),
            ColumnSort::Descending => self
                .items
                .sort_by(|left, right| Self::compare_tasks(kind, right, left)),
        }
    }
}

#[derive(Clone, Copy)]
enum ToolbarCommand {
    Resume,
    Pause,
    PauseAll,
    ResumeAll,
    Delete,
    Open,
    Reveal,
}

impl DownloadView {
    fn select_all_tasks(
        &mut self,
        _: &SelectAllTasks,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.table_state.update(cx, |table, cx| {
            table.delegate_mut().select_all_tasks();
            cx.notify();
        });
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "toolbar helper keeps per-button visual and command inputs explicit"
    )]
    fn toolbar_icon_action(
        &self,
        tooltip_id: &'static str,
        button_id: &'static str,
        label: SharedString,
        icon: IconName,
        destructive: bool,
        action: ToolbarCommand,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tooltip_label = label.clone();
        div()
            .id(tooltip_id)
            .size(px(30.))
            .tooltip(move |window, cx| Tooltip::new(tooltip_label.clone()).build(window, cx))
            .child(
                toolbar_action_button(
                    button_id,
                    label,
                    Icon::new(icon).size(px(15.)),
                    destructive,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.execute_toolbar(action, cx);
                })),
            )
    }

    fn execute_toolbar(&mut self, action: ToolbarCommand, cx: &mut Context<Self>) {
        let selected = self.table_state.read(cx).delegate().selected_task_ids();
        let commands = match action {
            ToolbarCommand::PauseAll => vec![DownloadsCommand::PauseAll],
            ToolbarCommand::ResumeAll => vec![DownloadsCommand::ResumeAll],
            ToolbarCommand::Open | ToolbarCommand::Reveal => selected
                .into_iter()
                .filter_map(|id| {
                    let (source, task_id) = id.split_once(':').unwrap_or(("local", id.as_str()));
                    (source == "local").then(|| match action {
                        ToolbarCommand::Open => DownloadsCommand::OpenTask {
                            task_id: task_id.to_owned(),
                        },
                        ToolbarCommand::Reveal => DownloadsCommand::RevealTask {
                            task_id: task_id.to_owned(),
                        },
                        _ => unreachable!(),
                    })
                })
                .collect(),
            ToolbarCommand::Resume | ToolbarCommand::Pause | ToolbarCommand::Delete => selected
                .into_iter()
                .map(|id| {
                    let (source, task_id) = id.split_once(':').unwrap_or(("local", id.as_str()));
                    if source == "remote" {
                        DownloadsCommand::RemoteCommand(serde_json::json!({
                            "taskId": task_id,
                            "action": match action {
                                ToolbarCommand::Resume => "resume",
                                ToolbarCommand::Pause => "pause",
                                ToolbarCommand::Delete => "delete",
                                ToolbarCommand::PauseAll
                                | ToolbarCommand::ResumeAll
                                | ToolbarCommand::Open
                                | ToolbarCommand::Reveal => unreachable!(),
                            }
                        }))
                    } else {
                        match action {
                            ToolbarCommand::Resume => DownloadsCommand::Resume {
                                task_id: task_id.to_owned(),
                            },
                            ToolbarCommand::Pause => DownloadsCommand::Pause {
                                task_id: task_id.to_owned(),
                            },
                            ToolbarCommand::Delete => DownloadsCommand::Delete {
                                task_id: task_id.to_owned(),
                                delete_files: false,
                            },
                            ToolbarCommand::PauseAll
                            | ToolbarCommand::ResumeAll
                            | ToolbarCommand::Open
                            | ToolbarCommand::Reveal => unreachable!(),
                        }
                    }
                })
                .collect(),
        };
        for command in commands {
            let future = self.controller.execute(command);
            cx.spawn(async move |this, cx| {
                let failed = future.await.is_err();
                let _ = this.update(cx, |this, cx| {
                    this.last_error = failed.then(|| this.strings.action_failed.clone());
                    cx.notify();
                });
            })
            .detach();
        }
    }

    fn render_columns_menu(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let table_state = self.table_state.clone();
        let menu_title = self.strings.view_columns_menu_title.clone();
        let reset_label = self.strings.view_columns_reset_action.clone();
        let at_least_one_label = self.strings.view_columns_at_least_one.clone();
        let trigger_label = menu_title.clone();

        Popover::new("download-columns-popover")
            .p_0()
            .w(px(230.))
            .trigger(
                Button::new("download-columns")
                    .ghost()
                    .xsmall()
                    .compact()
                    .icon(IconName::Settings2)
                    .tooltip(trigger_label),
            )
            .content(move |_, _, cx| {
                let tokens = active_theme(cx).tokens().clone();
                let columns = table_state.read(cx).delegate().columns.clone();
                let visible_count = columns.iter().filter(|column| column.visible).count();
                let table_for_reset = table_state.clone();

                v_flex()
                    .w_full()
                    .py(tokens.spacing.sm)
                    .child(
                        h_flex()
                            .px(tokens.spacing.md)
                            .pb(tokens.spacing.sm)
                            .gap(tokens.spacing.sm)
                            .text_size(tokens.typography.xs.size)
                            .font_weight(tokens.typography.sm.weight)
                            .child(Icon::new(IconName::Settings2).size(px(13.)))
                            .child(menu_title.clone()),
                    )
                    .children(columns.into_iter().map(|column| {
                        let kind = column.kind;
                        let label = kind.label(&table_state.read(cx).delegate().strings);
                        let drag = DraggedColumnMenuItem {
                            kind,
                            label: label.clone(),
                        };
                        let table_for_checkbox = table_state.clone();
                        let table_for_drop = table_state.clone();
                        h_flex()
                            .id(format!("download-column-menu-item-{}", kind.key()))
                            .h(px(32.))
                            .px(tokens.spacing.md)
                            .gap(tokens.spacing.sm)
                            .cursor_pointer()
                            .hover(|style| style.bg(tokens.colors.muted.opacity(0.55)))
                            .drag_over::<DraggedColumnMenuItem>(move |style, _, _, _| {
                                style.bg(tokens.colors.accent)
                            })
                            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                            .on_drop(move |drag: &DraggedColumnMenuItem, _, cx| {
                                table_for_drop.update(cx, |table, cx| {
                                    table.delegate_mut().move_column_kind(drag.kind, kind);
                                    table.refresh(cx);
                                    cx.notify();
                                });
                            })
                            .child(
                                Icon::new(IconName::Menu)
                                    .size(px(13.))
                                    .text_color(tokens.colors.muted_foreground),
                            )
                            .child(
                                Checkbox::new(format!("download-column-visible-{}", kind.key()))
                                    .with_size(Size::XSmall)
                                    .checked(column.visible)
                                    .focus_ring(false)
                                    .on_click(move |checked: &bool, _, cx| {
                                        table_for_checkbox.update(cx, |table, cx| {
                                            if table
                                                .delegate_mut()
                                                .set_column_visible(kind, *checked)
                                            {
                                                table.refresh(cx);
                                                cx.notify();
                                            }
                                        });
                                    }),
                            )
                            .child(div().min_w_0().flex_1().truncate().child(label))
                    }))
                    .when(visible_count == 1, |this| {
                        this.child(
                            div()
                                .px(tokens.spacing.md)
                                .py(tokens.spacing.xs)
                                .text_size(px(10.))
                                .text_color(tokens.colors.muted_foreground)
                                .child(at_least_one_label.clone()),
                        )
                    })
                    .child(
                        div()
                            .h(px(1.))
                            .my(tokens.spacing.xs)
                            .bg(tokens.colors.border),
                    )
                    .child(
                        Button::new("download-columns-reset")
                            .ghost()
                            .w_full()
                            .justify_start()
                            .label(reset_label.clone())
                            .on_click(move |_, _, cx| {
                                table_for_reset.update(cx, |table, cx| {
                                    table.delegate_mut().reset_columns();
                                    table.refresh(cx);
                                    cx.notify();
                                });
                            }),
                    )
            })
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let separator = || {
            div()
                .h(px(24.))
                .w(px(1.))
                .mx(tokens.spacing.xs)
                .bg(tokens.colors.border)
        };

        h_flex()
            .h(px(40.))
            .w_full()
            .flex_none()
            .items_center()
            .px(px(4.))
            .mb(px(4.))
            .gap(tokens.spacing.xxs)
            .child(
                Button::new("download-create")
                    .primary()
                    .small()
                    .label(self.strings.new_download.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(opener) = this.new_download_opener.clone() {
                            let context = this.new_download_context();
                            opener(context, window, cx);
                        }
                    })),
            )
            .child(separator())
            .child(self.toolbar_icon_action(
                "download-resume-tooltip",
                "download-resume",
                self.strings.resume.clone(),
                IconName::Play,
                false,
                ToolbarCommand::Resume,
                cx,
            ))
            .child(self.toolbar_icon_action(
                "download-pause-tooltip",
                "download-pause",
                self.strings.pause.clone(),
                IconName::Pause,
                false,
                ToolbarCommand::Pause,
                cx,
            ))
            .child(separator())
            .child(self.toolbar_icon_action(
                "download-stop-all-tooltip",
                "download-stop-all",
                self.strings.stop_all.clone(),
                IconName::CircleX,
                false,
                ToolbarCommand::PauseAll,
                cx,
            ))
            .child(self.toolbar_icon_action(
                "download-resume-all-tooltip",
                "download-resume-all",
                self.strings.resume.clone(),
                IconName::Play,
                false,
                ToolbarCommand::ResumeAll,
                cx,
            ))
            .child(separator())
            .child(self.toolbar_icon_action(
                "download-delete-tooltip",
                "download-delete",
                self.strings.delete.clone(),
                IconName::Delete,
                true,
                ToolbarCommand::Delete,
                cx,
            ))
            .child(self.toolbar_icon_action(
                "download-open-tooltip",
                "download-open",
                self.strings.open_file.clone(),
                IconName::File,
                false,
                ToolbarCommand::Open,
                cx,
            ))
            .child(self.toolbar_icon_action(
                "download-reveal-tooltip",
                "download-reveal",
                self.strings.open_folder.clone(),
                IconName::FolderOpen,
                false,
                ToolbarCommand::Reveal,
                cx,
            ))
            .child(div().flex_1())
            .child(self.render_columns_menu(cx))
    }

    fn resolve_selection(
        &mut self,
        request_id: String,
        outcome: fluxdown_protocol::SelectionOutcome,
        cx: &mut Context<Self>,
    ) {
        let future = self.controller.execute(DownloadsCommand::ResolveSelection(
            fluxdown_protocol::SelectionResolutionDto {
                request_id,
                outcome,
            },
        ));
        cx.spawn(async move |this, cx| {
            let failed = future.await.is_err();
            let _ = this.update(cx, |this, cx| {
                this.last_error = failed.then(|| this.strings.action_failed.clone());
                cx.notify();
            });
        })
        .detach();
    }

    fn render_pending_selection(
        &self,
        request: fluxdown_protocol::SelectionRequestDto,
        cx: &mut Context<Self>,
    ) -> Div {
        let tokens = active_theme(cx).tokens();
        let confirm_id = request.request_id.clone();
        let confirm_outcome = request.default_choice.clone();
        let cancel_id = request.request_id.clone();
        let cancellable = !matches!(request.kind, fluxdown_protocol::SelectionKind::Hls { .. });
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(tokens.colors.border)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_sm()
                    .truncate()
                    .child(request.task_id),
            )
            .child(
                Button::new(format!("selection-confirm-{confirm_id}"))
                    .primary()
                    .small()
                    .label(self.strings.confirm.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.resolve_selection(confirm_id.clone(), confirm_outcome.clone(), cx);
                    })),
            )
            .when(cancellable, |this| {
                this.child(
                    Button::new(format!("selection-cancel-{cancel_id}"))
                        .outline()
                        .small()
                        .label(self.strings.cancel.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.resolve_selection(
                                cancel_id.clone(),
                                fluxdown_protocol::SelectionOutcome::Cancelled,
                                cx,
                            );
                        })),
                )
            })
    }

    pub(crate) fn render_main(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(tokens.colors.surface)
            .child(self.render_toolbar(cx))
            .when_some(self.last_error.clone(), |this, error| {
                this.child(
                    div()
                        .w_full()
                        .px_3()
                        .py_2()
                        .border_b_1()
                        .border_color(tokens.colors.border)
                        .text_sm()
                        .child(error),
                )
            })
            .when_some(
                self.controller.pending_selection().cloned(),
                |this, request| this.child(self.render_pending_selection(request, cx)),
            )
            .child(
                div()
                    .id("download-task-table-container")
                    .relative()
                    .mx(px(4.))
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .on_action(cx.listener(Self::select_all_tasks))
                    .child(
                        div()
                            .absolute()
                            .top(px(-TABLE_HEADER_CROP))
                            .right_0()
                            .bottom_0()
                            .left_0()
                            .child(
                                DataTable::new(&self.table_state)
                                    .with_size(Size::Size(px(TASK_ROW_HEIGHT)))
                                    .stripe(false)
                                    .bordered(false)
                                    .scrollbar_visible(true, true),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::Arc};

    use fluxdown_ui_i18n::{I18nCatalog, I18nError};

    use super::DownloadTableDelegate;
    use crate::{model::DownloadTaskView, strings::DownloadStrings};

    #[test]
    fn context_menu_selection_preserves_selected_rows_and_replaces_unselected_rows()
    -> Result<(), I18nError> {
        let catalog = Arc::new(I18nCatalog::load_embedded()?);
        let strings = DownloadStrings::from_translator(&catalog.translator("en"));
        let tasks = (0..3)
            .map(|index| {
                let task =
                    serde_json::from_value::<fluxdown_protocol::TaskDto>(serde_json::json!({
                        "taskId": format!("t{index}"),
                        "url": "https://example.com/file",
                        "fileName": format!("f{index}.bin"),
                        "saveDir": "/tmp",
                        "status": 2,
                        "downloadedBytes": 0,
                        "totalBytes": 1,
                        "errorMessage": "",
                        "createdAt": "1",
                        "proxyUrl": "",
                        "queueId": "main",
                        "checksum": ""
                    }))
                    .expect("task");
                DownloadTaskView::local_with_speed(&task, None)
            })
            .collect();
        let mut delegate = DownloadTableDelegate::new(strings, tasks);
        delegate
            .selected_tasks
            .extend(["local:t0".to_owned(), "local:t1".to_owned()]);

        delegate.select_task_for_context_menu("local:t1".to_owned());
        assert_eq!(
            delegate.selected_tasks,
            HashSet::from(["local:t0".to_owned(), "local:t1".to_owned()])
        );

        delegate.select_task_for_context_menu("local:t2".to_owned());
        assert_eq!(
            delegate.selected_tasks,
            HashSet::from(["local:t2".to_owned()])
        );
        Ok(())
    }
}
