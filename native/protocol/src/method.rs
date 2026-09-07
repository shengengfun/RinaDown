//! 本机服务 JSON-RPC 方法名与能力标识。

pub const SYSTEM_HELLO: &str = "system.hello";
pub const SYSTEM_PING: &str = "system.ping";
pub const SYSTEM_SNAPSHOT: &str = "system.snapshot";

pub const DAEMON_TASK_LIST: &str = "daemon.task.list";
pub const DAEMON_TASK_GET: &str = "daemon.task.get";
pub const DAEMON_TASK_CREATE: &str = "daemon.task.create";
pub const DAEMON_TASK_PAUSE: &str = "daemon.task.pause";
pub const DAEMON_TASK_RESUME: &str = "daemon.task.resume";
pub const DAEMON_TASK_RENAME: &str = "daemon.task.rename";
pub const DAEMON_TASK_DELETE: &str = "daemon.task.delete";
pub const DAEMON_TASK_PAUSE_ALL: &str = "daemon.task.pauseAll";
pub const DAEMON_TASK_RESUME_ALL: &str = "daemon.task.resumeAll";
pub const DAEMON_TASK_RESCAN: &str = "daemon.task.rescan";
pub const DAEMON_TASK_SET_SEED_LIMITS: &str = "daemon.task.setSeedLimits";

pub const DAEMON_QUEUE_LIST: &str = "daemon.queue.list";
pub const DAEMON_QUEUE_CREATE: &str = "daemon.queue.create";
pub const DAEMON_QUEUE_UPDATE: &str = "daemon.queue.update";
pub const DAEMON_QUEUE_DELETE: &str = "daemon.queue.delete";
pub const DAEMON_QUEUE_START: &str = "daemon.queue.start";
pub const DAEMON_QUEUE_STOP: &str = "daemon.queue.stop";
pub const DAEMON_QUEUE_SCHEDULE: &str = "daemon.queue.schedule";
pub const DAEMON_QUEUE_REORDER: &str = "daemon.queue.reorder";
pub const DAEMON_QUEUE_MOVE_TASK: &str = "daemon.queue.moveTask";
pub const DAEMON_QUEUE_BOOST: &str = "daemon.queue.boost";

pub const DAEMON_GROUP_LIST: &str = "daemon.group.list";
pub const DAEMON_GROUP_RESOLVE_PREVIEW: &str = "daemon.group.resolvePreview";
pub const DAEMON_GROUP_CREATE: &str = "daemon.group.create";
pub const DAEMON_GROUP_PAUSE: &str = "daemon.group.pause";
pub const DAEMON_GROUP_RESUME: &str = "daemon.group.resume";
pub const DAEMON_GROUP_DELETE: &str = "daemon.group.delete";

pub const DAEMON_CONFIG_GET: &str = "daemon.config.get";
pub const DAEMON_CONFIG_PATCH: &str = "daemon.config.patch";
pub const DAEMON_CONFIG_PROXY_TEST: &str = "daemon.config.proxyTest";
pub const DAEMON_CONFIG_CONN_POLICY: &str = "daemon.config.connPolicy";
pub const DAEMON_CONFIG_CLEAR_CONN_POLICY: &str = "daemon.config.clearConnPolicy";
pub const DAEMON_SITE_AUTH_LIST: &str = "daemon.siteAuth.list";
pub const DAEMON_SITE_AUTH_DELETE: &str = "daemon.siteAuth.delete";
pub const DAEMON_SITE_AUTH_CLEAR: &str = "daemon.siteAuth.clear";
pub const DAEMON_RUNTIME_STATS: &str = "daemon.runtime.stats";
pub const DAEMON_FS_LIST: &str = "daemon.fs.list";

pub const DAEMON_RSS_LIST_SOURCES: &str = "daemon.rss.listSources";
pub const DAEMON_RSS_GET_ITEMS: &str = "daemon.rss.getItems";
pub const DAEMON_RSS_CREATE_SOURCE: &str = "daemon.rss.createSource";
pub const DAEMON_RSS_UPDATE_SOURCE: &str = "daemon.rss.updateSource";
pub const DAEMON_RSS_DELETE_SOURCE: &str = "daemon.rss.deleteSource";
pub const DAEMON_RSS_REFRESH_SOURCE: &str = "daemon.rss.refreshSource";
pub const DAEMON_RSS_ITEM_ACTION: &str = "daemon.rss.itemAction";
pub const DAEMON_RSS_VALIDATE: &str = "daemon.rss.validate";

pub const DAEMON_PLUGIN_LIST: &str = "daemon.plugin.list";
pub const DAEMON_PLUGIN_SET_ENABLED: &str = "daemon.plugin.setEnabled";
pub const DAEMON_PLUGIN_UPDATE_SETTINGS: &str = "daemon.plugin.updateSettings";
pub const DAEMON_PLUGIN_INSTALL: &str = "daemon.plugin.install";
pub const DAEMON_PLUGIN_INSTALL_DEV: &str = "daemon.plugin.installDev";
pub const DAEMON_PLUGIN_UNINSTALL: &str = "daemon.plugin.uninstall";
pub const DAEMON_PLUGIN_MARKET_LIST: &str = "daemon.plugin.marketList";
pub const DAEMON_PLUGIN_MARKET_INSTALL: &str = "daemon.plugin.marketInstall";
pub const DAEMON_PLUGIN_IGNORE_RETRY: &str = "daemon.plugin.ignoreRetry";
pub const DAEMON_COMPONENT_GET: &str = "daemon.component.get";
pub const DAEMON_COMPONENT_LIST_VERSIONS: &str = "daemon.component.listVersions";
pub const DAEMON_COMPONENT_INSTALL: &str = "daemon.component.install";
pub const DAEMON_COMPONENT_UNINSTALL: &str = "daemon.component.uninstall";

pub const DAEMON_SELECTION_SUBSCRIBE: &str = "daemon.selection.subscribe";
pub const DAEMON_SELECTION_UNSUBSCRIBE: &str = "daemon.selection.unsubscribe";
pub const DAEMON_SELECTION_RESOLVE: &str = "daemon.selection.resolve";

pub const DAEMON_WEBHOOK_GET: &str = "daemon.webhook.get";
pub const DAEMON_WEBHOOK_CLEAR_DELIVERIES: &str = "daemon.webhook.clearDeliveries";
pub const DAEMON_WEBHOOK_SIMULATE: &str = "daemon.webhook.simulate";
pub const DAEMON_WEBHOOK_TEST: &str = "daemon.webhook.test";
pub const DAEMON_CDN_REPORTS_PEEK: &str = "daemon.cdnReports.peek";
pub const DAEMON_CDN_REPORTS_ACK: &str = "daemon.cdnReports.ack";
pub const DAEMON_CDN_CONFIG_APPLY: &str = "daemon.cdnConfig.apply";

pub const DAEMON_BT_TRACKER_SUBSCRIPTION_REFRESH: &str = "daemon.bt.trackerSubscription.refresh";
pub const DAEMON_ED2K_SERVER_SUBSCRIPTION_REFRESH: &str = "daemon.ed2k.serverSubscription.refresh";
pub const DAEMON_DIAGNOSTICS_DESCRIBE: &str = "daemon.diagnostics.describe";
pub const DAEMON_DIAGNOSTICS_PREPARE_LOG_EXPORT: &str = "daemon.diagnostics.prepareLogExport";
pub const DAEMON_MIGRATION_LINK_EXPORT: &str = "daemon.migration.linkExport";
pub const DAEMON_MIGRATION_LINK_ACK: &str = "daemon.migration.linkAck";
pub const DAEMON_MIGRATION_GATEWAY_EXPORT: &str = "daemon.migration.gatewayExport";
pub const DAEMON_MIGRATION_GATEWAY_ACK: &str = "daemon.migration.gatewayAck";

pub const AGENT_SESSION_GET: &str = "agent.session.get";
pub const AGENT_AUTH_REGISTER: &str = "agent.auth.register";
pub const AGENT_AUTH_REGISTER_VERIFY: &str = "agent.auth.registerVerify";
pub const AGENT_AUTH_LOGIN: &str = "agent.auth.login";
pub const AGENT_AUTH_LOGIN_VERIFY: &str = "agent.auth.loginVerify";
pub const AGENT_AUTH_SEND_CODE: &str = "agent.auth.sendCode";
pub const AGENT_AUTH_VERIFY_CODE: &str = "agent.auth.verifyCode";
pub const AGENT_AUTH_LOGOUT: &str = "agent.auth.logout";
pub const AGENT_AUTH_REFRESH_PROFILE: &str = "agent.auth.refreshProfile";
pub const AGENT_PROFILE_SEND_EMAIL_CODE: &str = "agent.profile.sendEmailCode";
pub const AGENT_PROFILE_SEND_NEW_EMAIL_CODE: &str = "agent.profile.sendNewEmailCode";
pub const AGENT_PROFILE_CHANGE_EMAIL: &str = "agent.profile.changeEmail";
pub const AGENT_PROFILE_RANDOM_ORIGIN_ID: &str = "agent.profile.randomOriginId";
pub const AGENT_PROFILE_CHECK_ORIGIN_ID: &str = "agent.profile.checkOriginId";
pub const AGENT_PROFILE_CHANGE_ORIGIN_ID: &str = "agent.profile.changeOriginId";
pub const AGENT_PROFILE_CHANGE_NICKNAME: &str = "agent.profile.changeNickname";

pub const AGENT_GATEWAY_GET: &str = "agent.gateway.get";
pub const AGENT_GATEWAY_PATCH: &str = "agent.gateway.patch";
/// 仅供本机官方 UI 展示/复制用户 token；结果 `{ "userToken": "..." }`（未配置为空串）。
pub const AGENT_GATEWAY_REVEAL_TOKEN: &str = "agent.gateway.revealToken";
pub const AGENT_DEVICE_LIST: &str = "agent.device.list";
pub const AGENT_DEVICE_RENAME: &str = "agent.device.rename";
pub const AGENT_DEVICE_DELETE: &str = "agent.device.delete";
pub const AGENT_PREFERENCES_PATCH: &str = "agent.preferences.patch";
pub const AGENT_SYNC_GET: &str = "agent.sync.get";
pub const AGENT_SYNC_ENABLE: &str = "agent.sync.enable";
pub const AGENT_SYNC_DISABLE: &str = "agent.sync.disable";
pub const AGENT_SYNC_NOW: &str = "agent.sync.now";
pub const AGENT_REMOTE_LIST: &str = "agent.remote.list";
pub const AGENT_REMOTE_DISPATCH: &str = "agent.remote.dispatch";
pub const AGENT_REMOTE_COMMAND: &str = "agent.remote.command";

pub const AGENT_PLAN_LIST: &str = "agent.plan.list";
pub const AGENT_ORDER_CREATE: &str = "agent.order.create";
pub const AGENT_ORDER_GET: &str = "agent.order.get";
pub const AGENT_ORDER_LIST: &str = "agent.order.list";
pub const AGENT_REFERRAL_SUMMARY: &str = "agent.referral.summary";
pub const AGENT_REFERRAL_LIST_CODES: &str = "agent.referral.listCodes";
pub const AGENT_REFERRAL_CREATE_CODE: &str = "agent.referral.createCode";
pub const AGENT_REFERRAL_DELETE_CODE: &str = "agent.referral.deleteCode";
pub const AGENT_REFERRAL_LIST_RECORDS: &str = "agent.referral.listRecords";
pub const AGENT_REFERRAL_VALIDATE: &str = "agent.referral.validate";

pub const AGENT_PLATFORM_OPEN_TASK: &str = "agent.platform.openTask";
pub const AGENT_PLATFORM_REVEAL_TASK: &str = "agent.platform.revealTask";
pub const AGENT_PLATFORM_OPEN_PATH: &str = "agent.platform.openPath";
pub const AGENT_PLATFORM_INTEGRATION_GET: &str = "agent.platform.integrationGet";
pub const AGENT_PLATFORM_SET_AUTOSTART: &str = "agent.platform.setAutostart";
pub const AGENT_PLATFORM_SET_FILE_ASSOCIATION: &str = "agent.platform.setFileAssociation";
pub const AGENT_PLATFORM_SET_URL_PROTOCOL: &str = "agent.platform.setUrlProtocol";
pub const AGENT_CAPTURE_SUBMIT: &str = "agent.capture.submit";
/// 从本机 `.torrent` 文件建任务：agent 读文件、上传 daemon blob 后调用 `daemon.task.create`。
pub const AGENT_CAPTURE_SUBMIT_TORRENT_FILE: &str = "agent.capture.submitTorrentFile";
pub const AGENT_CAPTURE_LIST: &str = "agent.capture.list";
pub const AGENT_CAPTURE_RESOLVE: &str = "agent.capture.resolve";
/// 从本机插件包安装：agent 读文件、上传 daemon blob 后调用 `daemon.plugin.install`。
pub const AGENT_PLUGIN_INSTALL_FILE: &str = "agent.plugin.installFile";
pub const AGENT_DIAGNOSTICS_RUN: &str = "agent.diagnostics.run";
pub const AGENT_DIAGNOSTICS_REPAIR: &str = "agent.diagnostics.repair";
pub const AGENT_DIAGNOSTICS_LOG_PATHS: &str = "agent.diagnostics.logPaths";
pub const AGENT_DIAGNOSTICS_EXPORT_LOGS: &str = "agent.diagnostics.exportLogs";
pub const AGENT_UPDATE_CHECK: &str = "agent.update.check";

pub const SERVICE_EVENT: &str = "service.event";

pub const CAPABILITY_DAEMON_TASKS: &str = "daemon.tasks";
pub const CAPABILITY_DAEMON_QUEUES: &str = "daemon.queues";
pub const CAPABILITY_DAEMON_GROUPS: &str = "daemon.groups";
pub const CAPABILITY_DAEMON_CONFIG: &str = "daemon.config";
pub const CAPABILITY_DAEMON_RSS: &str = "daemon.rss";
pub const CAPABILITY_DAEMON_PLUGINS: &str = "daemon.plugins";
pub const CAPABILITY_DAEMON_COMPONENTS: &str = "daemon.components";
pub const CAPABILITY_DAEMON_WEBHOOKS: &str = "daemon.webhooks";
pub const CAPABILITY_DAEMON_SELECTIONS: &str = "daemon.selections";
pub const CAPABILITY_DAEMON_FILES: &str = "daemon.files";
pub const CAPABILITY_AGENT_GATEWAY: &str = "agent.gateway";
pub const CAPABILITY_AGENT_AUTH: &str = "agent.auth";
pub const CAPABILITY_AGENT_SYNC: &str = "agent.sync";
pub const CAPABILITY_AGENT_REMOTE_TASKS: &str = "agent.remoteTasks";
pub const CAPABILITY_AGENT_BILLING: &str = "agent.billing";
pub const CAPABILITY_AGENT_REFERRALS: &str = "agent.referrals";
pub const CAPABILITY_AGENT_DEVICE_LINK: &str = "agent.deviceLink";
pub const CAPABILITY_AGENT_EXTERNAL_CAPTURE: &str = "agent.externalCapture";
pub const CAPABILITY_CLIENT_SELECTIONS: &str = "client.selections";

/// 规范ALL_METHODS。
pub const ALL_METHODS: &[&str] = &[
    SYSTEM_HELLO,
    SYSTEM_PING,
    SYSTEM_SNAPSHOT,
    DAEMON_TASK_LIST,
    DAEMON_TASK_GET,
    DAEMON_TASK_CREATE,
    DAEMON_TASK_PAUSE,
    DAEMON_TASK_RESUME,
    DAEMON_TASK_RENAME,
    DAEMON_TASK_DELETE,
    DAEMON_TASK_PAUSE_ALL,
    DAEMON_TASK_RESUME_ALL,
    DAEMON_TASK_RESCAN,
    DAEMON_TASK_SET_SEED_LIMITS,
    DAEMON_QUEUE_LIST,
    DAEMON_QUEUE_CREATE,
    DAEMON_QUEUE_UPDATE,
    DAEMON_QUEUE_DELETE,
    DAEMON_QUEUE_START,
    DAEMON_QUEUE_STOP,
    DAEMON_QUEUE_SCHEDULE,
    DAEMON_QUEUE_REORDER,
    DAEMON_QUEUE_MOVE_TASK,
    DAEMON_QUEUE_BOOST,
    DAEMON_GROUP_LIST,
    DAEMON_GROUP_RESOLVE_PREVIEW,
    DAEMON_GROUP_CREATE,
    DAEMON_GROUP_PAUSE,
    DAEMON_GROUP_RESUME,
    DAEMON_GROUP_DELETE,
    DAEMON_CONFIG_GET,
    DAEMON_CONFIG_PATCH,
    DAEMON_CONFIG_PROXY_TEST,
    DAEMON_CONFIG_CONN_POLICY,
    DAEMON_CONFIG_CLEAR_CONN_POLICY,
    DAEMON_SITE_AUTH_LIST,
    DAEMON_SITE_AUTH_DELETE,
    DAEMON_SITE_AUTH_CLEAR,
    DAEMON_RUNTIME_STATS,
    DAEMON_FS_LIST,
    DAEMON_RSS_LIST_SOURCES,
    DAEMON_RSS_GET_ITEMS,
    DAEMON_RSS_CREATE_SOURCE,
    DAEMON_RSS_UPDATE_SOURCE,
    DAEMON_RSS_DELETE_SOURCE,
    DAEMON_RSS_REFRESH_SOURCE,
    DAEMON_RSS_ITEM_ACTION,
    DAEMON_RSS_VALIDATE,
    DAEMON_PLUGIN_LIST,
    DAEMON_PLUGIN_SET_ENABLED,
    DAEMON_PLUGIN_UPDATE_SETTINGS,
    DAEMON_PLUGIN_INSTALL,
    DAEMON_PLUGIN_INSTALL_DEV,
    DAEMON_PLUGIN_UNINSTALL,
    DAEMON_PLUGIN_MARKET_LIST,
    DAEMON_PLUGIN_MARKET_INSTALL,
    DAEMON_PLUGIN_IGNORE_RETRY,
    DAEMON_COMPONENT_GET,
    DAEMON_COMPONENT_LIST_VERSIONS,
    DAEMON_COMPONENT_INSTALL,
    DAEMON_COMPONENT_UNINSTALL,
    DAEMON_SELECTION_SUBSCRIBE,
    DAEMON_SELECTION_UNSUBSCRIBE,
    DAEMON_SELECTION_RESOLVE,
    DAEMON_WEBHOOK_GET,
    DAEMON_WEBHOOK_CLEAR_DELIVERIES,
    DAEMON_WEBHOOK_SIMULATE,
    DAEMON_WEBHOOK_TEST,
    DAEMON_CDN_REPORTS_PEEK,
    DAEMON_CDN_REPORTS_ACK,
    DAEMON_CDN_CONFIG_APPLY,
    DAEMON_BT_TRACKER_SUBSCRIPTION_REFRESH,
    DAEMON_ED2K_SERVER_SUBSCRIPTION_REFRESH,
    DAEMON_DIAGNOSTICS_DESCRIBE,
    DAEMON_DIAGNOSTICS_PREPARE_LOG_EXPORT,
    DAEMON_MIGRATION_LINK_EXPORT,
    DAEMON_MIGRATION_LINK_ACK,
    DAEMON_MIGRATION_GATEWAY_EXPORT,
    DAEMON_MIGRATION_GATEWAY_ACK,
    AGENT_SESSION_GET,
    AGENT_AUTH_REGISTER,
    AGENT_AUTH_REGISTER_VERIFY,
    AGENT_AUTH_LOGIN,
    AGENT_AUTH_LOGIN_VERIFY,
    AGENT_AUTH_SEND_CODE,
    AGENT_AUTH_VERIFY_CODE,
    AGENT_AUTH_LOGOUT,
    AGENT_AUTH_REFRESH_PROFILE,
    AGENT_PROFILE_SEND_EMAIL_CODE,
    AGENT_PROFILE_SEND_NEW_EMAIL_CODE,
    AGENT_PROFILE_CHANGE_EMAIL,
    AGENT_PROFILE_RANDOM_ORIGIN_ID,
    AGENT_PROFILE_CHECK_ORIGIN_ID,
    AGENT_PROFILE_CHANGE_ORIGIN_ID,
    AGENT_PROFILE_CHANGE_NICKNAME,
    AGENT_GATEWAY_GET,
    AGENT_GATEWAY_PATCH,
    AGENT_GATEWAY_REVEAL_TOKEN,
    AGENT_DEVICE_LIST,
    AGENT_DEVICE_RENAME,
    AGENT_DEVICE_DELETE,
    AGENT_PREFERENCES_PATCH,
    AGENT_SYNC_GET,
    AGENT_SYNC_ENABLE,
    AGENT_SYNC_DISABLE,
    AGENT_SYNC_NOW,
    AGENT_REMOTE_LIST,
    AGENT_REMOTE_DISPATCH,
    AGENT_REMOTE_COMMAND,
    AGENT_PLAN_LIST,
    AGENT_ORDER_CREATE,
    AGENT_ORDER_GET,
    AGENT_ORDER_LIST,
    AGENT_REFERRAL_SUMMARY,
    AGENT_REFERRAL_LIST_CODES,
    AGENT_REFERRAL_CREATE_CODE,
    AGENT_REFERRAL_DELETE_CODE,
    AGENT_REFERRAL_LIST_RECORDS,
    AGENT_REFERRAL_VALIDATE,
    AGENT_PLATFORM_OPEN_TASK,
    AGENT_PLATFORM_REVEAL_TASK,
    AGENT_PLATFORM_OPEN_PATH,
    AGENT_PLATFORM_INTEGRATION_GET,
    AGENT_PLATFORM_SET_AUTOSTART,
    AGENT_PLATFORM_SET_FILE_ASSOCIATION,
    AGENT_PLATFORM_SET_URL_PROTOCOL,
    AGENT_CAPTURE_SUBMIT,
    AGENT_CAPTURE_SUBMIT_TORRENT_FILE,
    AGENT_CAPTURE_LIST,
    AGENT_CAPTURE_RESOLVE,
    AGENT_PLUGIN_INSTALL_FILE,
    AGENT_DIAGNOSTICS_RUN,
    AGENT_DIAGNOSTICS_REPAIR,
    AGENT_DIAGNOSTICS_LOG_PATHS,
    AGENT_DIAGNOSTICS_EXPORT_LOGS,
    AGENT_UPDATE_CHECK,
    SERVICE_EVENT,
];

/// 规范DAEMON_CAPABILITIES。
pub const DAEMON_CAPABILITIES: &[&str] = &[
    CAPABILITY_DAEMON_TASKS,
    CAPABILITY_DAEMON_QUEUES,
    CAPABILITY_DAEMON_GROUPS,
    CAPABILITY_DAEMON_CONFIG,
    CAPABILITY_DAEMON_RSS,
    CAPABILITY_DAEMON_PLUGINS,
    CAPABILITY_DAEMON_COMPONENTS,
    CAPABILITY_DAEMON_WEBHOOKS,
    CAPABILITY_DAEMON_SELECTIONS,
    CAPABILITY_DAEMON_FILES,
];

/// 规范AGENT_CAPABILITIES。
pub const AGENT_CAPABILITIES: &[&str] = &[
    CAPABILITY_AGENT_GATEWAY,
    CAPABILITY_AGENT_AUTH,
    CAPABILITY_AGENT_SYNC,
    CAPABILITY_AGENT_REMOTE_TASKS,
    CAPABILITY_AGENT_BILLING,
    CAPABILITY_AGENT_REFERRALS,
    CAPABILITY_AGENT_DEVICE_LINK,
    CAPABILITY_AGENT_EXTERNAL_CAPTURE,
];

/// 规范CLIENT_CAPABILITIES。
pub const CLIENT_CAPABILITIES: &[&str] = &[CAPABILITY_CLIENT_SELECTIONS];
