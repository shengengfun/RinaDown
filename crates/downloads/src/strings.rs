use fluxdown_ui_i18n::{Translator, keys};
use gpui::SharedString;

#[derive(Clone)]
pub(crate) struct DownloadStrings {
    pub(crate) category_archive: SharedString,
    pub(crate) category_audio: SharedString,
    pub(crate) category_document: SharedString,
    pub(crate) category_image: SharedString,
    pub(crate) category_other: SharedString,
    pub(crate) category_program: SharedString,
    pub(crate) category_video: SharedString,
    pub(crate) col_created: SharedString,
    pub(crate) col_eta: SharedString,
    pub(crate) col_file_name: SharedString,
    pub(crate) col_size: SharedString,
    pub(crate) col_speed: SharedString,
    pub(crate) col_status: SharedString,
    pub(crate) delete: SharedString,
    pub(crate) confirm: SharedString,
    pub(crate) cancel: SharedString,
    pub(crate) disconnected: SharedString,
    pub(crate) action_failed: SharedString,
    pub(crate) metadata_loading: SharedString,
    eta_seconds: SharedString,
    eta_minutes: SharedString,
    eta_hours: SharedString,
    pub(crate) later_queue: SharedString,
    pub(crate) pause: SharedString,
    pub(crate) resume: SharedString,
    pub(crate) main_queue: SharedString,
    pub(crate) new_download: SharedString,
    pub(crate) open_file: SharedString,
    pub(crate) open_folder: SharedString,
    pub(crate) sidebar_queues: SharedString,
    pub(crate) status_all: SharedString,
    pub(crate) status_completed: SharedString,
    pub(crate) status_incomplete: SharedString,
    pub(crate) status_paused: SharedString,
    pub(crate) stop_all: SharedString,
    pub(crate) today: SharedString,
    pub(crate) view_columns_at_least_one: SharedString,
    pub(crate) view_columns_menu_title: SharedString,
    pub(crate) view_columns_reset_action: SharedString,
}

impl DownloadStrings {
    pub(crate) fn from_translator(translator: &Translator) -> Self {
        Self {
            category_archive: shared(translator.text(keys::CATEGORY_ARCHIVE)),
            category_audio: shared(translator.text(keys::CATEGORY_AUDIO)),
            category_document: shared(translator.text(keys::CATEGORY_DOCUMENT)),
            category_image: shared(translator.text(keys::CATEGORY_IMAGE)),
            category_other: shared(translator.text(keys::CATEGORY_OTHER)),
            category_program: shared(translator.text(keys::CATEGORY_PROGRAM)),
            category_video: shared(translator.text(keys::CATEGORY_VIDEO)),
            col_created: shared(translator.text(keys::COL_CREATED)),
            col_eta: shared(translator.text(keys::COL_ETA)),
            col_file_name: shared(translator.text(keys::COL_FILE_NAME)),
            col_size: shared(translator.text(keys::COL_SIZE)),
            col_speed: shared(translator.text(keys::COL_SPEED)),
            confirm: shared(translator.text("confirm")),
            cancel: shared(translator.text("cancel")),
            disconnected: shared(translator.text("localServiceDisconnected")),
            action_failed: shared(translator.text("localServiceActionFailed")),
            metadata_loading: shared(translator.text("statusPreparing")),
            eta_seconds: shared(translator.text("etaSeconds")),
            eta_minutes: shared(translator.text("etaMinutes")),
            eta_hours: shared(translator.text("etaHours")),
            col_status: shared(translator.text(keys::COL_STATUS)),
            later_queue: shared(translator.text(keys::LATER_QUEUE)),
            delete: shared(translator.text(keys::DELETE)),
            main_queue: shared(translator.text(keys::MAIN_QUEUE)),
            pause: shared(translator.text(keys::PAUSE)),
            resume: shared(translator.text(keys::RESUME)),
            new_download: shared(translator.text(keys::NEW_DOWNLOAD)),
            open_file: shared(translator.text("openFile")),
            open_folder: shared(translator.text("openFolder")),
            sidebar_queues: shared(translator.text(keys::SIDEBAR_QUEUES)),
            status_all: shared(translator.text(keys::TAB_ALL)),
            status_completed: shared(translator.text(keys::STATUS_COMPLETED)),
            status_incomplete: shared(translator.text(keys::STATUS_INCOMPLETE)),
            status_paused: shared(translator.text(keys::STATUS_PAUSED)),
            stop_all: shared(translator.text(keys::STOP_ALL)),
            today: shared(translator.text(keys::TODAY)),
            view_columns_at_least_one: shared(translator.text(keys::VIEW_COLUMNS_AT_LEAST_ONE)),
            view_columns_menu_title: shared(translator.text(keys::VIEW_COLUMNS_MENU_TITLE)),
            view_columns_reset_action: shared(translator.text(keys::VIEW_COLUMNS_RESET_ACTION)),
        }
    }

    pub(crate) fn format_eta(&self, seconds: u64) -> SharedString {
        let (template, value) = if seconds < 60 {
            (&self.eta_seconds, seconds.to_string())
        } else if seconds < 3600 {
            (&self.eta_minutes, (seconds / 60).to_string())
        } else {
            (&self.eta_hours, format!("{:.1}", seconds as f64 / 3600.0))
        };
        SharedString::from(template.replace("{n}", &value))
    }
}

/// 「新建下载」表单文案，键集与 `lib/src/widgets/new_download_dialog.dart` 一致。
#[derive(Clone)]
pub(crate) struct NewDownloadStrings {
    pub(crate) title: SharedString,
    pub(crate) subtitle: SharedString,
    pub(crate) url_label: SharedString,
    pub(crate) url_placeholder: SharedString,
    url_count: SharedString,
    pub(crate) no_valid_url: SharedString,
    pub(crate) open_torrent: SharedString,
    pub(crate) select_torrent: SharedString,
    pub(crate) import_txt: SharedString,
    pub(crate) import_txt_none: SharedString,
    import_txt_found: SharedString,
    pub(crate) save_dir: SharedString,
    pub(crate) save_dir_placeholder: SharedString,
    pub(crate) browse: SharedString,
    pub(crate) threads: SharedString,
    pub(crate) threads_auto: SharedString,
    pub(crate) threads_custom: SharedString,
    pub(crate) threads_custom_hint: SharedString,
    pub(crate) rename: SharedString,
    pub(crate) rename_placeholder: SharedString,
    pub(crate) advanced: SharedString,
    pub(crate) http_auth: SharedString,
    pub(crate) http_auth_desc: SharedString,
    pub(crate) http_auth_user: SharedString,
    pub(crate) http_auth_password: SharedString,
    pub(crate) http_auth_save: SharedString,
    pub(crate) proxy: SharedString,
    pub(crate) proxy_desc: SharedString,
    pub(crate) proxy_placeholder: SharedString,
    pub(crate) proxy_follow: SharedString,
    pub(crate) proxy_direct: SharedString,
    pub(crate) proxy_system: SharedString,
    pub(crate) proxy_global_manual: SharedString,
    pub(crate) proxy_custom: SharedString,
    pub(crate) proxy_not_configured: SharedString,
    pub(crate) ignore_tls: SharedString,
    pub(crate) ignore_tls_desc: SharedString,
    pub(crate) user_agent: SharedString,
    pub(crate) user_agent_desc: SharedString,
    pub(crate) ua_inherit: SharedString,
    pub(crate) ua_chrome: SharedString,
    pub(crate) ua_firefox: SharedString,
    pub(crate) ua_edge: SharedString,
    pub(crate) ua_safari: SharedString,
    pub(crate) ua_custom: SharedString,
    pub(crate) cookie: SharedString,
    pub(crate) cookie_desc: SharedString,
    pub(crate) cookie_placeholder: SharedString,
    pub(crate) checksum: SharedString,
    pub(crate) checksum_desc: SharedString,
    pub(crate) checksum_placeholder: SharedString,
    pub(crate) headers: SharedString,
    pub(crate) headers_desc: SharedString,
    pub(crate) header_name: SharedString,
    pub(crate) header_value: SharedString,
    pub(crate) add_header: SharedString,
    pub(crate) cancel: SharedString,
    pub(crate) download_later: SharedString,
    pub(crate) start_download: SharedString,
    start_batch: SharedString,
    later_tooltip: SharedString,
    start_tooltip: SharedString,
    pub(crate) main_queue: SharedString,
    pub(crate) later_queue: SharedString,
}

impl NewDownloadStrings {
    pub(crate) fn from_translator(translator: &Translator) -> Self {
        Self {
            title: shared(translator.text(keys::NEW_DOWNLOAD)),
            subtitle: shared(translator.text("batchDownloadDesc")),
            url_label: shared(translator.text("downloadUrl")),
            url_placeholder: shared(translator.text("batchUrlPlaceholder")),
            url_count: shared(translator.text("urlCount")),
            no_valid_url: shared(translator.text("newDownloadNoValidUrl")),
            open_torrent: shared(translator.text("openTorrentFile")),
            select_torrent: shared(translator.text("selectTorrentFile")),
            import_txt: shared(translator.text("importTxtFile")),
            import_txt_none: shared(translator.text("importTxtNoUrls")),
            import_txt_found: shared(translator.text("importTxtFound")),
            save_dir: shared(translator.text("saveDir")),
            save_dir_placeholder: shared(translator.text("selectSaveDir")),
            browse: shared(translator.text("browse")),
            threads: shared(translator.text("threads")),
            threads_auto: shared(translator.text("auto")),
            threads_custom: shared(translator.text("customThreads")),
            threads_custom_hint: shared(translator.text("customThreadsHint")),
            rename: shared(translator.text("renameOptional")),
            rename_placeholder: shared(translator.text("autoDetectFilename")),
            advanced: shared(translator.text("taskProxyAdvanced")),
            http_auth: shared(translator.text("taskHttpAuth")),
            http_auth_desc: shared(translator.text("taskHttpAuthDesc")),
            http_auth_user: shared(translator.text("taskHttpAuthUser")),
            http_auth_password: shared(translator.text("taskHttpAuthPassword")),
            http_auth_save: shared(translator.text("taskHttpAuthSaveForSite")),
            proxy: shared(translator.text("taskProxy")),
            proxy_desc: shared(translator.text("taskProxyDesc")),
            proxy_placeholder: shared(translator.text("taskProxyPlaceholder")),
            proxy_follow: shared(translator.text("taskProxyChoiceFollow")),
            proxy_direct: shared(translator.text("taskProxyChoiceDirect")),
            proxy_system: shared(translator.text("taskProxyChoiceSystem")),
            proxy_global_manual: shared(translator.text("taskProxyChoiceGlobalManual")),
            proxy_custom: shared(translator.text("taskProxyChoiceCustom")),
            proxy_not_configured: shared(translator.text("proxyNotConfigured")),
            ignore_tls: shared(translator.text("taskIgnoreTlsErrors")),
            ignore_tls_desc: shared(translator.text("taskIgnoreTlsErrorsDesc")),
            user_agent: shared(translator.text("userAgent")),
            user_agent_desc: shared(translator.text("userAgentTaskPlaceholder")),
            ua_inherit: shared(translator.text("queueUaInheritGlobal")),
            ua_chrome: shared(translator.text("userAgentPresetChrome")),
            ua_firefox: shared(translator.text("userAgentPresetFirefox")),
            ua_edge: shared(translator.text("userAgentPresetEdge")),
            ua_safari: shared(translator.text("userAgentPresetSafari")),
            ua_custom: shared(translator.text("userAgentPresetCustom")),
            cookie: shared(translator.text("taskCookie")),
            cookie_desc: shared(translator.text("taskCookieDesc")),
            cookie_placeholder: shared(translator.text("taskCookiePlaceholder")),
            checksum: shared(translator.text("taskChecksum")),
            checksum_desc: shared(translator.text("taskChecksumDesc")),
            checksum_placeholder: shared(translator.text("taskChecksumPlaceholder")),
            headers: shared(translator.text("taskHeaders")),
            headers_desc: shared(translator.text("taskHeadersDesc")),
            header_name: shared(translator.text("taskHeadersKeyPlaceholder")),
            header_value: shared(translator.text("taskHeadersValuePlaceholder")),
            add_header: shared(translator.text("taskHeadersAdd")),
            cancel: shared(translator.text("cancel")),
            download_later: shared(translator.text("downloadLater")),
            start_download: shared(translator.text("startDownload")),
            start_batch: shared(translator.text("startBatchDownload")),
            later_tooltip: shared(translator.text("laterIntoQueueTooltip")),
            start_tooltip: shared(translator.text("startIntoQueueTooltip")),
            main_queue: shared(translator.text(keys::MAIN_QUEUE)),
            later_queue: shared(translator.text(keys::LATER_QUEUE)),
        }
    }

    /// `{count} 个链接`。
    pub(crate) fn format_url_count(&self, count: usize) -> SharedString {
        SharedString::from(self.url_count.replace("{count}", &count.to_string()))
    }

    /// `已导入 {count} 个链接`。
    pub(crate) fn format_import_found(&self, count: usize) -> SharedString {
        SharedString::from(self.import_txt_found.replace("{count}", &count.to_string()))
    }

    /// 开始按钮标签：单条 = `开始下载`，多条 = `下载 {count} 个文件`。
    pub(crate) fn format_start(&self, count: usize) -> SharedString {
        if count > 1 {
            SharedString::from(self.start_batch.replace("{count}", &count.to_string()))
        } else {
            self.start_download.clone()
        }
    }

    /// 「稍后下载」按钮提示：`创建任务但不开始，加入「{name}」…`。
    pub(crate) fn format_later_tooltip(&self, queue_name: &str) -> SharedString {
        SharedString::from(self.later_tooltip.replace("{name}", queue_name))
    }

    /// 「开始下载」按钮提示：`下载到「{name}」…`。
    pub(crate) fn format_start_tooltip(&self, queue_name: &str) -> SharedString {
        SharedString::from(self.start_tooltip.replace("{name}", queue_name))
    }

    /// 队列显示名：内置队列本地化，自定义队列用用户命名。
    pub(crate) fn queue_name(&self, queue_id: &str, name: &str) -> SharedString {
        match queue_id {
            fluxdown_protocol::MAIN_QUEUE_ID => self.main_queue.clone(),
            fluxdown_protocol::LATER_QUEUE_ID => self.later_queue.clone(),
            _ => shared(name),
        }
    }
}

fn shared(value: &str) -> SharedString {
    SharedString::from(value.to_owned())
}
