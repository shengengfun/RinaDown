use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    components::task_table::DownloadTableDelegate,
    controller::{
        DownloadsCommand, DownloadsController, DownloadsPort, LAST_SAVE_DIR_PREF,
        REMEMBER_LAST_SAVE_DIR_PREF,
    },
    model::{
        DownloadFilter, DownloadStatusFilter, SidebarSelection, StatusFolderMotion,
        new_download::manual_proxy_url,
    },
    pages::new_download::{NewDownloadContext, NewDownloadQueue, NewDownloadSubmission},
    strings::DownloadStrings,
};
use fluxdown_ui_i18n::Translator;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, KeyBinding, ParentElement, Render,
    SharedString, Styled, Window, actions, div, px,
};
use gpui_component::{
    ResizableState, h_resizable, resizable_panel,
    table::{TableEvent, TableState},
};

actions!(downloads, [SelectAllTasks]);

pub(crate) const TASK_ROW_HEIGHT: f32 = 38.;
pub(crate) const SIDEBAR_MOTION_DURATION: Duration = Duration::from_millis(200);

/// 宿主注入的「新建下载」入口：由 app 打开独立对话框窗口。
/// 打开「新建下载」窗口；表单初值由下载页在点击瞬间算好传入，
/// 打开方不得再回读 `DownloadView`（此时实体正处于 update 中）。
pub type NewDownloadOpener = Rc<dyn Fn(NewDownloadContext, &mut Window, &mut App)>;

/// 下载能力的顶层页面。
pub struct DownloadView {
    pub(crate) controller: DownloadsController,
    pub(crate) strings: DownloadStrings,
    pub(crate) selected_item: SidebarSelection,
    pub(crate) expanded_status: Option<DownloadStatusFilter>,
    pub(crate) folder_motion: StatusFolderMotion,
    pub(crate) folder_motion_started_at: Option<Instant>,
    pub(crate) queues_expanded: bool,
    pub(crate) queue_motion_from: f32,
    pub(crate) queue_motion_started_at: Option<Instant>,
    pub(crate) table_state: Entity<TableState<DownloadTableDelegate>>,
    pub(crate) new_download_opener: Option<NewDownloadOpener>,
    pub(crate) last_error: Option<SharedString>,
    resizable_state: Entity<ResizableState>,
    resizable_state_initialized: bool,
}

impl DownloadView {
    /// 创建下载页面，并订阅共享翻译状态。
    pub fn new(
        translator: Entity<Translator>,
        port: Arc<dyn DownloadsPort>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.bind_keys([KeyBinding::new("ctrl-a", SelectAllTasks, Some("DataTable"))]);
        let strings = DownloadStrings::from_translator(translator.read(cx));
        let table_state = cx.new(|cx| {
            TableState::new(
                DownloadTableDelegate::new(strings.clone(), Vec::new()),
                window,
                cx,
            )
            .row_selectable(false)
            .col_selectable(false)
        });

        cx.observe(&translator, |this, translator, cx| {
            this.set_strings(DownloadStrings::from_translator(translator.read(cx)), cx);
        })
        .detach();
        cx.subscribe_in(&table_state, window, Self::handle_table_event)
            .detach();

        Self {
            controller: DownloadsController::new(port),
            strings,
            selected_item: SidebarSelection::Download(DownloadFilter::ALL),
            expanded_status: Some(DownloadStatusFilter::All),
            folder_motion: StatusFolderMotion::settled(Some(DownloadStatusFilter::All)),
            folder_motion_started_at: None,
            queues_expanded: true,
            queue_motion_from: 1.,
            queue_motion_started_at: None,
            table_state,
            new_download_opener: None,
            last_error: None,
            resizable_state: cx.new(|_| ResizableState::default()),
            resizable_state_initialized: false,
        }
    }

    /// 注入「新建下载」对话框的打开方式；未注入时工具栏按钮无动作。
    pub fn set_new_download_opener(&mut self, opener: NewDownloadOpener) {
        self.new_download_opener = Some(opener);
    }

    /// 「新建下载」表单的环境快照：保存目录 / 默认队列 / 线程数初值与队列候选。
    ///
    /// 与 Dart 一致：偏好 `remember_last_save_dir` 开启且有记录时沿用上次目录，
    /// 否则用全局默认；队列优先侧栏当前筛选，其次配置 `default_queue_id`，
    /// 最后主队列；线程数优先队列 `default_segments`，其次全局配置。
    #[must_use]
    pub fn new_download_context(&self) -> NewDownloadContext {
        let controller = &self.controller;
        let remember = controller
            .preference(REMEMBER_LAST_SAVE_DIR_PREF)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let last_save_dir = controller
            .preference(LAST_SAVE_DIR_PREF)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let save_dir = if remember && !last_save_dir.is_empty() {
            last_save_dir
        } else {
            controller.effective_save_dir()
        };
        let queue_id = match self.selected_item {
            SidebarSelection::MainQueue => fluxdown_protocol::MAIN_QUEUE_ID,
            SidebarSelection::LaterQueue => fluxdown_protocol::LATER_QUEUE_ID,
            SidebarSelection::Download(_) => match controller.config_str("default_queue_id") {
                "" => fluxdown_protocol::MAIN_QUEUE_ID,
                configured => configured,
            },
        };
        let queue_segments = controller
            .queues()
            .iter()
            .find(|queue| queue.queue_id == queue_id)
            .map_or(0, |queue| queue.default_segments);
        let segments = if queue_segments > 0 {
            queue_segments
        } else {
            controller
                .config_str("default_segments")
                .parse::<i32>()
                .unwrap_or(0)
        };
        NewDownloadContext {
            save_dir: save_dir.to_owned(),
            queue_id: queue_id.to_owned(),
            segments,
            queues: controller
                .queues()
                .iter()
                .map(|queue| NewDownloadQueue {
                    id: queue.queue_id.clone(),
                    name: queue.name.clone(),
                })
                .collect(),
            manual_proxy_url: manual_proxy_url(controller.config()),
        }
    }

    /// 按表单提交创建任务；对话框确认后由宿主调用。
    ///
    /// 链接逐条 `daemon.task.create`，任一失败即在页面横幅提示；同时把本次
    /// 保存目录记入本机偏好（无条件记录，开关开启后立即生效）。种子文件交给
    /// agent 读取上传。
    pub fn create_download(&mut self, submission: NewDownloadSubmission, cx: &mut Context<Self>) {
        if self.controller.is_stale() {
            return;
        }
        let futures = match submission {
            NewDownloadSubmission::Tasks(requests) => {
                if let Some(save_dir) = requests.first().map(|request| request.save_dir.clone()) {
                    // 记录目录是尽力而为：失败不影响任务创建，也不进横幅。
                    let remember = self
                        .controller
                        .execute(DownloadsCommand::RememberSaveDir { save_dir });
                    cx.background_spawn(async move {
                        let _ = remember.await;
                    })
                    .detach();
                }
                requests
                    .into_iter()
                    .map(|request| {
                        self.controller.execute(DownloadsCommand::Create(Box::new(
                            fluxdown_protocol::DaemonCreateTaskParams {
                                request,
                                torrent_blob_id: None,
                                unattended: false,
                            },
                        )))
                    })
                    .collect::<Vec<_>>()
            }
            NewDownloadSubmission::TorrentFiles(paths) => paths
                .iter()
                .map(|path| {
                    self.controller
                        .execute(DownloadsCommand::SubmitTorrentFile {
                            path: path.display().to_string(),
                        })
                })
                .collect(),
        };
        cx.spawn(async move |this, cx| {
            let mut failed = false;
            for future in futures {
                failed |= future.await.is_err();
            }
            let _ = this.update(cx, |this, cx| {
                this.last_error = failed.then(|| this.strings.action_failed.clone());
                cx.notify();
            });
        })
        .detach();
    }

    pub fn replace_snapshot(
        &mut self,
        snapshot: &fluxdown_protocol::AgentSnapshot,
        cx: &mut Context<Self>,
    ) {
        self.controller.replace_snapshot(snapshot);
        self.last_error = None;
        self.refresh_tasks(cx);
    }

    pub fn apply_event(&mut self, event: &fluxdown_protocol::ServiceEvent, cx: &mut Context<Self>) {
        self.controller.apply_event(event);
        self.refresh_tasks(cx);
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.last_error = Some(self.strings.disconnected.clone());
        self.controller.mark_stale();
        cx.notify();
    }

    fn refresh_tasks(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_item;
        let tasks = self.controller.tasks().to_vec();
        self.table_state.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.set_items(tasks);
            match selected {
                SidebarSelection::Download(filter) => delegate.set_filter(filter),
                SidebarSelection::MainQueue => {
                    delegate.set_queue_filter(fluxdown_protocol::MAIN_QUEUE_ID)
                }
                SidebarSelection::LaterQueue => {
                    delegate.set_queue_filter(fluxdown_protocol::LATER_QUEUE_ID)
                }
            }
            table.refresh(cx);
        });
        cx.notify();
    }

    fn set_strings(&mut self, strings: DownloadStrings, cx: &mut Context<Self>) {
        self.strings = strings.clone();
        self.table_state.update(cx, |table, cx| {
            table.delegate_mut().set_strings(strings);
            table.refresh(cx);
        });
        cx.notify();
    }

    fn handle_table_event(
        &mut self,
        table_state: &Entity<TableState<DownloadTableDelegate>>,
        event: &TableEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let TableEvent::RightClickedRow(Some(row_ix)) = event else {
            return;
        };
        table_state.update(cx, |table, cx| {
            if let Some(task_id) = table.delegate().task_id_at(*row_ix) {
                table.delegate_mut().select_task_for_context_menu(task_id);
            }
            table.set_right_clicked_row(None, cx);
        });
    }
}

impl Render for DownloadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main_panel_measured = self
            .resizable_state
            .read(cx)
            .sizes()
            .get(1)
            .is_some_and(|size| *size > px(1.));
        if !self.resizable_state_initialized && main_panel_measured {
            self.resizable_state
                .update(cx, |state, cx| state.reset_panel(1, cx));
            self.resizable_state_initialized = true;
        }

        div().size_full().min_w_0().min_h_0().child(
            h_resizable("downloads-content")
                .with_state(&self.resizable_state)
                .on_resize(|state, _, cx| {
                    state.update(cx, |state, cx| state.reset_panel(1, cx));
                })
                .child(
                    resizable_panel()
                        .size(px(160.))
                        .flex_none()
                        .size_range(px(148.)..px(280.))
                        .child(self.render_sidebar(window, cx)),
                )
                .child(resizable_panel().child(self.render_main(cx))),
        )
    }
}
