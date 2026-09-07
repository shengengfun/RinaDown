//! daemon RPC 方法分发与 actor 命令桥接。

#[cfg(feature = "plugins")]
use std::collections::HashMap;
#[cfg(any(feature = "plugins", feature = "components"))]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(feature = "components")]
use std::sync::atomic::{AtomicBool, Ordering};

use fluxdown_engine::download_manager::{CreateGroupSpec, GroupItemSpec};
use fluxdown_protocol::method;
use fluxdown_protocol::{
    ApplicationErrorCode, CdnConfigApplyParams, CdnReportAckParams, CreateGroupRequest,
    CreateQueueRequest, DaemonConfigPatch, DaemonCreateTaskParams, MigrationAckParams,
    RpcErrorData, RpcErrorObject, RpcRequest, RpcResponse, SelectionResolutionDto, ServiceHello,
    SiteAuthDeleteParams, SnapshotBody,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::actor::{ActorCallError, ActorError, ActorOperation, ActorResult, DaemonActorHandle};
use crate::blob_store::{BlobKind, BlobStore};
use crate::event_hub::DaemonEventHub;
use crate::selection::DaemonSelection;

/// daemon 运行时共享服务。
pub struct DaemonService {
    hello: ServiceHello,
    events: DaemonEventHub,
    selections: DaemonSelection,
    db: fluxdown_engine::db::Db,
    #[cfg(any(feature = "plugins", feature = "components"))]
    data_dir: PathBuf,
    #[cfg(feature = "plugins")]
    plugin_manager: Option<Arc<fluxdown_engine::plugin::PluginManager>>,
    blobs: Arc<BlobStore>,
    #[cfg(feature = "components")]
    ffmpeg_installing: AtomicBool,
    #[cfg(feature = "components")]
    ytdlp_installing: AtomicBool,
    actor: DaemonActorHandle,
}

impl DaemonService {
    #[allow(
        clippy::too_many_arguments,
        reason = "the daemon composition root passes each stateful service dependency explicitly"
    )]
    #[must_use]
    pub fn new(
        hello: ServiceHello,
        events: DaemonEventHub,
        selections: DaemonSelection,
        blobs: Arc<BlobStore>,
        actor: DaemonActorHandle,
        db: fluxdown_engine::db::Db,
        #[cfg(any(feature = "plugins", feature = "components"))] data_dir: PathBuf,
        #[cfg(feature = "plugins")] plugin_manager: Option<
            Arc<fluxdown_engine::plugin::PluginManager>,
        >,
    ) -> Self {
        Self {
            hello,
            events,
            selections,
            blobs,
            db,
            #[cfg(any(feature = "plugins", feature = "components"))]
            data_dir,
            #[cfg(feature = "plugins")]
            plugin_manager,
            #[cfg(feature = "components")]
            ffmpeg_installing: AtomicBool::new(false),
            #[cfg(feature = "components")]
            ytdlp_installing: AtomicBool::new(false),
            actor,
        }
    }

    #[must_use]
    pub fn hello(&self) -> &ServiceHello {
        &self.hello
    }

    #[must_use]
    pub fn events(&self) -> &DaemonEventHub {
        &self.events
    }

    #[must_use]
    pub fn selections(&self) -> &DaemonSelection {
        &self.selections
    }

    #[must_use]
    pub fn blobs(&self) -> &Arc<BlobStore> {
        &self.blobs
    }

    /// 刷新仅在完整引擎初始化后可计算的动态投影。
    pub async fn initialize_dynamic_projection(&self) -> Result<(), RpcErrorObject> {
        #[cfg(feature = "plugins")]
        self.publish_plugins().await?;
        #[cfg(feature = "components")]
        self.publish_component_statuses().await;
        let stats = self.runtime_stats().await;
        self.events
            .publish(fluxdown_protocol::DaemonEvent::RuntimeStatsChanged(stats));
        Ok(())
    }

    /// 从线性化投影读取任务，用于二进制文件端点。
    #[must_use]
    pub fn task(&self, task_id: &str) -> Option<fluxdown_protocol::TaskDto> {
        self.daemon_snapshot()
            .tasks
            .into_iter()
            .find(|task| task.task_id == task_id)
    }

    /// 分发 hello 后的连接级 RPC 调用。
    pub async fn call(
        &self,
        connection_id: &str,
        is_local_agent: bool,
        request: RpcRequest,
    ) -> RpcResponse {
        let id = request.id.clone();
        match self
            .call_inner(
                connection_id,
                is_local_agent,
                &request.method,
                request.params,
            )
            .await
        {
            Ok(result) => RpcResponse::success(id, result),
            Err(error) => RpcResponse::failure(id, error),
        }
    }

    async fn call_inner(
        &self,
        connection_id: &str,
        is_local_agent: bool,
        method_name: &str,
        params: Option<Value>,
    ) -> Result<Value, RpcErrorObject> {
        match method_name {
            method::SYSTEM_PING => Ok(json!({ "ok": true })),
            method::SYSTEM_SNAPSHOT => to_value(self.events.snapshot()),
            method::DAEMON_TASK_LIST => to_value(self.daemon_snapshot().tasks),
            method::DAEMON_TASK_GET => {
                let params = parse_params::<IdParams>(params)?;
                let task = self
                    .daemon_snapshot()
                    .tasks
                    .into_iter()
                    .find(|task| task.task_id == params.id)
                    .ok_or_else(not_found)?;
                to_value(task)
            }
            method::DAEMON_TASK_CREATE => self.create_task(params).await,
            method::DAEMON_TASK_PAUSE => {
                let params = parse_params::<IdParams>(params)?;
                self.execute_unit(ActorOperation::PauseTask { task_id: params.id })
                    .await
            }
            method::DAEMON_TASK_RESUME => {
                let params = parse_params::<IdParams>(params)?;
                self.execute_unit(ActorOperation::ResumeTask { task_id: params.id })
                    .await
            }
            method::DAEMON_TASK_RENAME => {
                let params = parse_params::<RenameParams>(params)?;
                self.execute_unit(ActorOperation::RenameTask {
                    task_id: params.task_id,
                    file_name: params.file_name,
                })
                .await
            }
            method::DAEMON_TASK_DELETE => {
                let params = parse_params::<DeleteParams>(params)?;
                self.execute_unit(ActorOperation::DeleteTask {
                    task_id: params.id,
                    delete_files: params.delete_files,
                })
                .await
            }
            method::DAEMON_TASK_PAUSE_ALL => self.execute_unit(ActorOperation::PauseAll).await,
            method::DAEMON_TASK_RESUME_ALL => self.execute_unit(ActorOperation::ResumeAll).await,
            method::DAEMON_TASK_RESCAN => self.execute_unit(ActorOperation::RescanFiles).await,
            method::DAEMON_TASK_SET_SEED_LIMITS => {
                let params = parse_params::<SeedLimitsParams>(params)?;
                self.execute_unit(ActorOperation::SetTaskSeedLimits {
                    task_id: params.task_id,
                    ratio_limit_milli: params.ratio_limit_milli,
                    post_ratio_limit_milli: params.post_ratio_limit_milli,
                    seed_time_limit_minutes: params.seed_time_limit_minutes,
                    inactive_time_limit_minutes: params.inactive_time_limit_minutes,
                    upload_limit_bps: params.upload_limit_bps,
                })
                .await
            }
            method::DAEMON_QUEUE_LIST => to_value(self.daemon_snapshot().queues),
            method::DAEMON_QUEUE_CREATE => {
                let request = parse_params::<CreateQueueRequest>(params)?;
                self.execute_unit(ActorOperation::CreateQueue {
                    name: request.name,
                    speed_limit_kbps: request.speed_limit_kbps,
                    upload_limit_kbps: request.upload_limit_kbps,
                    max_concurrent: request.max_concurrent,
                    default_save_dir: request.default_save_dir,
                    default_segments: request.default_segments,
                    default_user_agent: request.default_user_agent,
                })
                .await
            }
            method::DAEMON_QUEUE_UPDATE => {
                let params = parse_params::<UpdateQueueParams>(params)?;
                self.execute_unit(ActorOperation::UpdateQueue {
                    queue_id: params.queue_id,
                    name: params.request.name,
                    speed_limit_kbps: params.request.speed_limit_kbps,
                    upload_limit_kbps: params.request.upload_limit_kbps,
                    max_concurrent: params.request.max_concurrent,
                    default_save_dir: params.request.default_save_dir,
                    default_segments: params.request.default_segments,
                    default_user_agent: params.request.default_user_agent,
                })
                .await
            }
            method::DAEMON_QUEUE_DELETE => {
                self.queue_id_operation(params, QueueAction::Delete).await
            }
            method::DAEMON_QUEUE_START => self.queue_id_operation(params, QueueAction::Start).await,
            method::DAEMON_QUEUE_STOP => self.queue_id_operation(params, QueueAction::Stop).await,
            method::DAEMON_QUEUE_SCHEDULE => {
                let params = parse_params::<QueueScheduleParams>(params)?;
                self.execute_unit(ActorOperation::SetQueueSchedule {
                    queue_id: params.queue_id,
                    enabled: params.enabled,
                    start_time: params.start_time,
                    stop_time: params.stop_time,
                    days: params.days,
                })
                .await
            }
            method::DAEMON_QUEUE_REORDER => {
                let params = parse_params::<QueueOrderParams>(params)?;
                self.execute_unit(ActorOperation::ReorderQueue {
                    queue_id: params.queue_id,
                    task_ids: params.task_ids,
                })
                .await
            }
            method::DAEMON_QUEUE_MOVE_TASK => {
                let params = parse_params::<MoveTaskParams>(params)?;
                self.execute_unit(ActorOperation::MoveToQueue {
                    task_id: params.task_id,
                    queue_id: params.queue_id,
                })
                .await
            }
            method::DAEMON_QUEUE_BOOST => {
                let params = parse_params::<IdParams>(params)?;
                self.execute_unit(ActorOperation::Boost { task_id: params.id })
                    .await
            }
            method::DAEMON_GROUP_LIST => to_value(self.daemon_snapshot().groups),
            method::DAEMON_GROUP_RESOLVE_PREVIEW => {
                let request = parse_params::<fluxdown_protocol::ResolvePreviewRequest>(params)?;
                #[cfg(feature = "plugins")]
                {
                    let source_url = request.url.clone();
                    match self
                        .actor
                        .execute(ActorOperation::ResolvePreview {
                            url: request.url,
                            cookies: request.cookies,
                            referrer: request.referrer,
                            user_agent: request.user_agent,
                            extra_headers: request.extra_headers,
                        })
                        .await
                    {
                        Ok(ActorResult::ResolvePreview(outcome)) => {
                            to_value(fluxdown_protocol::ResolvePreviewResponse {
                                name: outcome.name,
                                source_url,
                                error: outcome.error,
                                items: outcome
                                    .items
                                    .into_iter()
                                    .map(manifest_item_to_preview_dto)
                                    .collect(),
                            })
                        }
                        Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                        Err(error) => Err(actor_error(error)),
                    }
                }
                #[cfg(not(feature = "plugins"))]
                {
                    let _ = request;
                    Err(unsupported_error("resolve preview requires plugins"))
                }
            }
            method::DAEMON_GROUP_CREATE => {
                let request = parse_params::<CreateGroupRequest>(params)?;
                let spec = self.group_spec(request);
                match self
                    .actor
                    .execute(ActorOperation::CreateGroup {
                        spec: Box::new(spec),
                    })
                    .await
                {
                    Ok(ActorResult::Created(group_id)) => to_value(json!({ "groupId": group_id })),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_GROUP_PAUSE => self.group_id_operation(params, GroupAction::Pause).await,
            method::DAEMON_GROUP_RESUME => {
                self.group_id_operation(params, GroupAction::Resume).await
            }
            method::DAEMON_GROUP_DELETE => {
                let params = parse_params::<DeleteParams>(params)?;
                self.execute_unit(ActorOperation::DeleteGroup {
                    group_id: params.id,
                    delete_files: params.delete_files,
                })
                .await
            }
            method::DAEMON_CONFIG_GET => to_value(self.daemon_snapshot().config),
            method::DAEMON_CONFIG_PATCH => {
                let patch = parse_params::<DaemonConfigPatch>(params)?;
                let values = crate::config::validate_config_patch(&patch.values)
                    .map_err(|error| invalid_argument("values", &error.to_string()))?;
                match self
                    .actor
                    .execute(ActorOperation::PatchConfig {
                        expected_revision: patch.expected_revision,
                        values,
                    })
                    .await
                {
                    Ok(ActorResult::Config(snapshot)) => to_value(snapshot),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_CONFIG_PROXY_TEST => {
                let params = parse_params::<ProxyTestParams>(params)?;
                match self
                    .actor
                    .execute(ActorOperation::TestProxy {
                        proxy_type: params.proxy_type,
                        host: params.host,
                        port: params.port,
                        username: params.username,
                        password: params.password,
                    })
                    .await
                {
                    Ok(ActorResult::ProxyLatency(latency_ms)) => {
                        to_value(json!({ "latencyMs": latency_ms }))
                    }
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_CONFIG_CONN_POLICY => {
                let raw = self
                    .db
                    .get_config("domain_conn_caps")
                    .await
                    .map_err(|error| internal_error(format!("{error:#}")))?
                    .unwrap_or_default();
                let count = fluxdown_engine::segment_coordinator::count_domain_conn_policies(&raw);
                to_value(fluxdown_protocol::ConnPolicySummaryDto {
                    domain_count: u64::try_from(count).unwrap_or(u64::MAX),
                })
            }
            method::DAEMON_CONFIG_CLEAR_CONN_POLICY => {
                match self.actor.execute(ActorOperation::ClearConnPolicy).await {
                    Ok(ActorResult::ConnPolicy(summary)) => to_value(summary),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_SITE_AUTH_LIST => {
                let json = self
                    .db
                    .get_config(fluxdown_engine::site_auth::SITE_AUTH_CONFIG_KEY)
                    .await
                    .map_err(|error| internal_error(format!("{error:#}")))?
                    .unwrap_or_default();
                to_value(crate::actor::site_auth_entries(
                    &fluxdown_engine::site_auth::parse_store(&json),
                ))
            }
            method::DAEMON_SITE_AUTH_DELETE => {
                let params = parse_params::<SiteAuthDeleteParams>(params)?;
                if params.site.trim().is_empty() {
                    return Err(invalid_argument("site", "site is required"));
                }
                self.site_auth_operation(ActorOperation::SiteAuthDelete { site: params.site })
                    .await
            }
            method::DAEMON_SITE_AUTH_CLEAR => {
                self.site_auth_operation(ActorOperation::SiteAuthClear)
                    .await
            }
            method::DAEMON_RSS_LIST_SOURCES => {
                let sources = self
                    .db
                    .load_all_rss_sources()
                    .await
                    .map_err(|error| internal_error(format!("{error:#}")))?
                    .into_iter()
                    .map(fluxdown_engine_protocol::rss_source_info_to_dto)
                    .collect::<Vec<_>>();
                to_value(sources)
            }
            method::DAEMON_RSS_GET_ITEMS => {
                let params = parse_params::<RssSourceIdParams>(params)?;
                let items = self
                    .db
                    .load_rss_items(
                        &params.source_id,
                        fluxdown_engine::rss::MAX_ITEMS_PER_SOURCE,
                    )
                    .await
                    .map_err(|error| internal_error(format!("{error:#}")))?
                    .into_iter()
                    .map(fluxdown_engine_protocol::rss_item_info_to_dto)
                    .collect::<Vec<_>>();
                to_value(items)
            }
            method::DAEMON_RSS_CREATE_SOURCE => {
                let source = parse_params::<fluxdown_protocol::RssSourceDto>(params)?;
                match self
                    .actor
                    .execute(ActorOperation::RssCreate {
                        source: Box::new(fluxdown_engine_protocol::rss_source_dto_to_engine(
                            source,
                        )),
                    })
                    .await
                {
                    Ok(ActorResult::Created(source_id)) => {
                        to_value(json!({ "sourceId": source_id }))
                    }
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_RSS_UPDATE_SOURCE => {
                let source = parse_params::<fluxdown_protocol::RssSourceDto>(params)?;
                let source_id = source.source_id.clone();
                if source_id.trim().is_empty() {
                    return Err(invalid_argument("sourceId", "sourceId is required"));
                }
                match self
                    .actor
                    .execute(ActorOperation::RssUpdate {
                        source: Box::new(fluxdown_engine_protocol::rss_source_dto_to_engine(
                            source,
                        )),
                    })
                    .await
                {
                    Ok(ActorResult::Boolean(true)) => Ok(json!({ "ok": true })),
                    Ok(ActorResult::Boolean(false)) => Err(not_found()),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_RSS_DELETE_SOURCE => {
                let params = parse_params::<RssSourceIdParams>(params)?;
                self.rss_boolean_operation(ActorOperation::RssDelete {
                    source_id: params.source_id,
                })
                .await
            }
            method::DAEMON_RSS_REFRESH_SOURCE => {
                let params = parse_params::<RssSourceIdParams>(params)?;
                self.rss_boolean_operation(ActorOperation::RssRefresh {
                    source_id: params.source_id,
                })
                .await
            }
            method::DAEMON_RSS_ITEM_ACTION => {
                let params = parse_params::<RssItemActionParams>(params)?;
                self.execute_unit(ActorOperation::RssItemAction {
                    source_id: params.source_id,
                    guid: params.guid,
                    action: params.action,
                })
                .await
            }
            method::DAEMON_RSS_VALIDATE => {
                let request = parse_params::<fluxdown_protocol::RssValidateRequest>(params)?;
                match self
                    .actor
                    .execute(ActorOperation::RssValidate {
                        url: request.url,
                        cookies: request.cookies,
                        user_agent: request.user_agent,
                        proxy_url: request.proxy_url,
                    })
                    .await
                {
                    Ok(ActorResult::RssValidation(outcome)) => {
                        to_value(fluxdown_protocol::RssValidateResponse {
                            url: outcome.url,
                            feed_title: outcome.feed_title,
                            items: outcome
                                .items
                                .into_iter()
                                .map(fluxdown_engine_protocol::rss_item_info_to_dto)
                                .collect(),
                            error: outcome.error,
                        })
                    }
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_RUNTIME_STATS => to_value(self.runtime_stats().await),
            method::DAEMON_FS_LIST => {
                let params = parse_optional_params::<FsListParams>(params)?;
                self.list_directories(params.path).await.and_then(to_value)
            }
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_LIST => to_value(self.list_plugins().await?),
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_SET_ENABLED => {
                let params = parse_params::<PluginEnabledParams>(params)?;
                let manager = self.plugin_manager()?;
                manager
                    .set_enabled(&params.identity, params.enabled)
                    .await
                    .map_err(|error| invalid_argument("identity", &error.to_string()))?;
                self.publish_plugins().await?;
                Ok(json!({ "ok": true }))
            }
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_UPDATE_SETTINGS => {
                let params = parse_params::<PluginSettingsParams>(params)?;
                let entries = params.entries.into_iter().collect::<Vec<_>>();
                self.plugin_manager()?
                    .update_settings(&params.identity, &entries)
                    .await
                    .map_err(|error| invalid_argument("entries", &error.to_string()))?;
                self.publish_plugins().await?;
                Ok(json!({ "ok": true }))
            }
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_INSTALL => {
                let params = parse_params::<PluginInstallParams>(params)?;
                let bytes = self
                    .blobs
                    .read(&params.blob_id, BlobKind::Plugin)
                    .await
                    .map_err(|error| invalid_argument("blobId", &error.to_string()))?;
                let identity = self
                    .plugin_manager()?
                    .install_from_zip(bytes)
                    .await
                    .map_err(|error| invalid_argument("blobId", &error.to_string()))?;
                self.blobs
                    .consume(&params.blob_id, BlobKind::Plugin)
                    .await
                    .map_err(|error| internal_error(format!("{error:#}")))?;
                let missing_components = self.plugin_missing_components(&identity).await;
                self.publish_plugins().await?;
                to_value(fluxdown_protocol::InstalledPlugin {
                    identity,
                    missing_components,
                })
            }
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_INSTALL_DEV => {
                let request = parse_params::<fluxdown_protocol::InstallPluginDevRequest>(params)?;
                let identity = self
                    .plugin_manager()?
                    .install_dev(std::path::Path::new(&request.dir_path))
                    .await
                    .map_err(|error| invalid_argument("dirPath", &error.to_string()))?;
                let missing_components = self.plugin_missing_components(&identity).await;
                self.publish_plugins().await?;
                to_value(fluxdown_protocol::InstalledPlugin {
                    identity,
                    missing_components,
                })
            }
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_UNINSTALL => {
                let params = parse_params::<PluginIdentityParams>(params)?;
                self.plugin_manager()?
                    .uninstall(&params.identity)
                    .await
                    .map_err(|error| invalid_argument("identity", &error.to_string()))?;
                self.publish_plugins().await?;
                Ok(json!({ "ok": true }))
            }
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_MARKET_LIST => {
                let index = self
                    .market_client()
                    .await?
                    .fetch_index()
                    .await
                    .map_err(|error| invalid_argument("market", &error.to_string()))?;
                to_value(
                    index
                        .entries
                        .into_iter()
                        .map(fluxdown_engine_protocol::market_entry_to_dto)
                        .collect::<Vec<_>>(),
                )
            }
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_MARKET_INSTALL => {
                let request = parse_params::<fluxdown_protocol::MarketInstallRequest>(params)?;
                let identity = self
                    .market_client()
                    .await?
                    .install_latest(&request.plugin_id)
                    .await
                    .map_err(|error| invalid_argument("pluginId", &error.to_string()))?;
                let missing_components = self.plugin_missing_components(&identity).await;
                self.publish_plugins().await?;
                to_value(fluxdown_protocol::InstalledPlugin {
                    identity,
                    missing_components,
                })
            }
            #[cfg(feature = "plugins")]
            method::DAEMON_PLUGIN_IGNORE_RETRY => {
                let params = parse_params::<TaskIdParams>(params)?;
                if self.task(&params.task_id).is_none() {
                    return Err(not_found());
                }
                self.plugin_manager()?
                    .clear_task_resolver(&params.task_id)
                    .await;
                self.execute_unit(ActorOperation::ResumeTask {
                    task_id: params.task_id,
                })
                .await
            }
            #[cfg(not(feature = "plugins"))]
            method::DAEMON_PLUGIN_LIST
            | method::DAEMON_PLUGIN_SET_ENABLED
            | method::DAEMON_PLUGIN_UPDATE_SETTINGS
            | method::DAEMON_PLUGIN_INSTALL
            | method::DAEMON_PLUGIN_INSTALL_DEV
            | method::DAEMON_PLUGIN_UNINSTALL
            | method::DAEMON_PLUGIN_MARKET_LIST
            | method::DAEMON_PLUGIN_MARKET_INSTALL
            | method::DAEMON_PLUGIN_IGNORE_RETRY => {
                Err(unsupported_error("daemon was built without plugin support"))
            }
            #[cfg(feature = "components")]
            method::DAEMON_COMPONENT_GET => {
                let params = parse_params::<fluxdown_protocol::ComponentParams>(params)?;
                to_value(self.component_status(params.component).await)
            }
            #[cfg(feature = "components")]
            method::DAEMON_COMPONENT_LIST_VERSIONS => {
                let params = parse_params::<fluxdown_protocol::ComponentParams>(params)?;
                to_value(self.component_versions(params.component).await?)
            }
            #[cfg(feature = "components")]
            method::DAEMON_COMPONENT_INSTALL => {
                let params = parse_params::<fluxdown_protocol::ComponentInstallParams>(params)?;
                self.install_component(params.component, params.version)
                    .await
            }
            #[cfg(feature = "components")]
            method::DAEMON_COMPONENT_UNINSTALL => {
                let params = parse_params::<fluxdown_protocol::ComponentParams>(params)?;
                self.uninstall_component(params.component).await
            }
            #[cfg(not(feature = "components"))]
            method::DAEMON_COMPONENT_GET
            | method::DAEMON_COMPONENT_LIST_VERSIONS
            | method::DAEMON_COMPONENT_INSTALL
            | method::DAEMON_COMPONENT_UNINSTALL => Err(unsupported_error(
                "daemon was built without component support",
            )),
            method::DAEMON_WEBHOOK_GET => {
                match self.actor.execute(ActorOperation::WebhookDeliveries).await {
                    Ok(ActorResult::WebhookDeliveries(deliveries)) => {
                        to_value(fluxdown_protocol::WebhookDeliveriesResponse {
                            deliveries: deliveries
                                .into_iter()
                                .map(fluxdown_engine_protocol::webhook_delivery_to_dto)
                                .collect(),
                            presets: fluxdown_engine::webhook::preset_catalog()
                                .into_iter()
                                .map(fluxdown_engine_protocol::webhook_preset_to_dto)
                                .collect(),
                            variables: fluxdown_engine::webhook::TEMPLATE_VARIABLES
                                .iter()
                                .map(|variable| (*variable).to_owned())
                                .collect(),
                        })
                    }
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_WEBHOOK_CLEAR_DELIVERIES => {
                self.execute_unit(ActorOperation::WebhookClear).await
            }
            method::DAEMON_WEBHOOK_SIMULATE => {
                match self.actor.execute(ActorOperation::WebhookSimulate).await {
                    Ok(ActorResult::WebhookSimulation(dispatched)) => {
                        to_value(fluxdown_protocol::WebhookSimulateResponse {
                            dispatched: i32::try_from(dispatched).unwrap_or(i32::MAX),
                        })
                    }
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_WEBHOOK_TEST => {
                let request = parse_params::<fluxdown_protocol::WebhookTestRequest>(params)?;
                match self
                    .actor
                    .execute(ActorOperation::WebhookTest {
                        endpoint_json: request.endpoint.to_string(),
                    })
                    .await
                {
                    Ok(ActorResult::WebhookTest(delivery)) => {
                        to_value(fluxdown_protocol::WebhookTestResponse {
                            success: delivery.success,
                            status_code: delivery.status_code,
                            latency_ms: delivery.latency_ms,
                            error: delivery.error.clone(),
                        })
                    }
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_CDN_REPORTS_PEEK => {
                match self.actor.execute(ActorOperation::CdnReportsPeek).await {
                    Ok(ActorResult::CdnLease(lease)) => to_value(lease),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_CDN_REPORTS_ACK => {
                let params = parse_params::<CdnReportAckParams>(params)?;
                match self
                    .actor
                    .execute(ActorOperation::CdnReportsAck {
                        batch_id: params.batch_id,
                    })
                    .await
                {
                    Ok(ActorResult::Boolean(true)) => Ok(json!({ "ok": true })),
                    Ok(ActorResult::Boolean(false)) => Err(RpcErrorObject::application(
                        "CDN report lease does not match",
                        RpcErrorData::new(ApplicationErrorCode::Conflict, false),
                    )),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_CDN_CONFIG_APPLY => {
                let params = parse_params::<CdnConfigApplyParams>(params)?;
                self.execute_unit(ActorOperation::CdnConfigApply {
                    values: params.values,
                })
                .await
            }
            method::DAEMON_BT_TRACKER_SUBSCRIPTION_REFRESH => {
                match self
                    .actor
                    .execute(ActorOperation::RefreshTrackerSubscription)
                    .await
                {
                    Ok(ActorResult::TrackerRefresh(response)) => to_value(response),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_ED2K_SERVER_SUBSCRIPTION_REFRESH => {
                match self
                    .actor
                    .execute(ActorOperation::RefreshEd2kServerSubscription)
                    .await
                {
                    Ok(ActorResult::Ed2kRefresh(response)) => to_value(response),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_DIAGNOSTICS_DESCRIBE => {
                let snapshot = self.daemon_snapshot();
                Ok(json!({
                    "service": self.hello,
                    "tasks": snapshot.tasks.len(),
                    "queues": snapshot.queues.len(),
                    "groups": snapshot.groups.len(),
                    "configRevision": snapshot.config.revision,
                    "logDir": fluxdown_engine::logger::log_dir().display().to_string(),
                    "components": snapshot.components,
                }))
            }
            method::DAEMON_DIAGNOSTICS_PREPARE_LOG_EXPORT => {
                let snapshot = self.events.snapshot();
                let bytes = serde_json::to_vec(&snapshot)
                    .map_err(|error| internal_error(error.to_string()))?;
                let export_id = self
                    .blobs
                    .put(BlobKind::Logs, &bytes)
                    .await
                    .map_err(|error| internal_error(format!("{error:#}")))?;
                Ok(json!({ "exportId": export_id }))
            }
            method::DAEMON_MIGRATION_LINK_EXPORT => {
                require_local_agent(is_local_agent)?;
                match self
                    .actor
                    .execute(ActorOperation::MigrationLinkExport)
                    .await
                {
                    Ok(ActorResult::LinkMigration(export)) => to_value(export),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_MIGRATION_LINK_ACK => {
                require_local_agent(is_local_agent)?;
                let params = parse_params::<MigrationAckParams>(params)?;
                self.execute_unit(ActorOperation::MigrationLinkAck {
                    revision: params.revision,
                })
                .await
            }
            method::DAEMON_MIGRATION_GATEWAY_EXPORT => {
                require_local_agent(is_local_agent)?;
                match self
                    .actor
                    .execute(ActorOperation::MigrationGatewayExport)
                    .await
                {
                    Ok(ActorResult::GatewayMigration(export)) => to_value(export),
                    Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
                    Err(error) => Err(actor_error(error)),
                }
            }
            method::DAEMON_MIGRATION_GATEWAY_ACK => {
                require_local_agent(is_local_agent)?;
                let params = parse_params::<MigrationAckParams>(params)?;
                self.execute_unit(ActorOperation::MigrationGatewayAck {
                    revision: params.revision,
                })
                .await
            }
            method::DAEMON_SELECTION_SUBSCRIBE => {
                self.selections.subscribe(connection_id.to_owned());
                Ok(json!({ "ok": true }))
            }
            method::DAEMON_SELECTION_UNSUBSCRIBE => {
                self.selections.unsubscribe(connection_id);
                Ok(json!({ "ok": true }))
            }
            method::DAEMON_SELECTION_RESOLVE => {
                let resolution = parse_params::<SelectionResolutionDto>(params)?;
                self.selections.resolve(resolution).map_err(|error| {
                    let code = match error {
                        crate::selection::SelectionError::NotFound => {
                            ApplicationErrorCode::NotFound
                        }
                        crate::selection::SelectionError::Conflict => {
                            ApplicationErrorCode::Conflict
                        }
                        crate::selection::SelectionError::InvalidOutcome
                        | crate::selection::SelectionError::HlsCancel => {
                            ApplicationErrorCode::InvalidArgument
                        }
                    };
                    RpcErrorObject::application(error.to_string(), RpcErrorData::new(code, false))
                })?;
                Ok(json!({ "ok": true }))
            }
            _ => Err(RpcErrorObject::application(
                "method is not supported by this daemon build",
                RpcErrorData::new(ApplicationErrorCode::Unsupported, false),
            )),
        }
    }

    async fn create_task(&self, params: Option<Value>) -> Result<Value, RpcErrorObject> {
        let params = parse_params::<DaemonCreateTaskParams>(params)?;
        if params.torrent_blob_id.is_some() && params.request.torrent_b64.is_some() {
            return Err(invalid_argument(
                "torrentBlobId",
                "torrentBlobId and torrentB64 are mutually exclusive",
            ));
        }
        let bytes = match params.torrent_blob_id.as_deref() {
            Some(blob_id) => {
                self.blobs
                    .read(blob_id, BlobKind::Torrent)
                    .await
                    .map_err(|error| {
                        RpcErrorObject::application(
                            error.to_string(),
                            RpcErrorData::new(ApplicationErrorCode::NotFound, false),
                        )
                    })?
            }
            None => Vec::new(),
        };
        let result = self
            .actor
            .execute(ActorOperation::CreateTask {
                request: Box::new(params.request),
                torrent_file_bytes: bytes,
                hint_file_size: 0,
                unattended: params.unattended,
            })
            .await;
        match result {
            Ok(ActorResult::Created(task_id)) => {
                if let Some(blob_id) = params.torrent_blob_id {
                    self.blobs
                        .consume(&blob_id, BlobKind::Torrent)
                        .await
                        .map_err(|error| internal_error(format!("{error:#}")))?;
                }
                to_value(json!({ "taskId": task_id }))
            }
            Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
            Err(error) => Err(actor_error(error)),
        }
    }

    async fn execute_unit(&self, operation: ActorOperation) -> Result<Value, RpcErrorObject> {
        match self.actor.execute(operation).await {
            Ok(ActorResult::Unit) => Ok(json!({ "ok": true })),
            Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
            Err(error) => Err(actor_error(error)),
        }
    }

    /// 站点凭据表写操作：返回更新后的脱敏列表。
    async fn site_auth_operation(
        &self,
        operation: ActorOperation,
    ) -> Result<Value, RpcErrorObject> {
        match self.actor.execute(operation).await {
            Ok(ActorResult::SiteAuth(entries)) => to_value(entries),
            Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
            Err(error) => Err(actor_error(error)),
        }
    }

    async fn queue_id_operation(
        &self,
        params: Option<Value>,
        action: QueueAction,
    ) -> Result<Value, RpcErrorObject> {
        let params = parse_params::<IdParams>(params)?;
        let operation = match action {
            QueueAction::Delete => ActorOperation::DeleteQueue {
                queue_id: params.id,
            },
            QueueAction::Start => ActorOperation::StartQueue {
                queue_id: params.id,
            },
            QueueAction::Stop => ActorOperation::StopQueue {
                queue_id: params.id,
            },
        };
        self.execute_unit(operation).await
    }

    async fn group_id_operation(
        &self,
        params: Option<Value>,
        action: GroupAction,
    ) -> Result<Value, RpcErrorObject> {
        let params = parse_params::<IdParams>(params)?;
        let operation = match action {
            GroupAction::Pause => ActorOperation::PauseGroup {
                group_id: params.id,
            },
            GroupAction::Resume => ActorOperation::ResumeGroup {
                group_id: params.id,
            },
        };
        self.execute_unit(operation).await
    }

    fn daemon_snapshot(&self) -> fluxdown_protocol::DaemonSnapshot {
        match self.events.snapshot().body {
            SnapshotBody::Daemon(snapshot) => *snapshot,
            SnapshotBody::Agent(_) => unreachable!("daemon event hub returned agent snapshot"),
        }
    }

    fn group_spec(&self, request: CreateGroupRequest) -> CreateGroupSpec {
        let base_save_dir = if request.save_dir.trim().is_empty() {
            self.daemon_snapshot()
                .config
                .values
                .get("default_save_dir")
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .unwrap_or_else(fluxdown_engine::user_dirs::download_dir_or_cwd)
        } else {
            request.save_dir
        };
        CreateGroupSpec {
            source_url: request.source_url,
            group_name: request.group_name,
            base_save_dir,
            queue_id: request.queue_id,
            segments: request.segments,
            cookies: request.cookies,
            referrer: request.referrer,
            user_agent: request.user_agent,
            proxy_url: request.proxy_url,
            extra_headers: request.extra_headers,
            ignore_tls_errors: request.ignore_tls_errors,
            start_paused: request.start_paused,
            items: request
                .items
                .into_iter()
                .map(|item| GroupItemSpec {
                    resolver_item: item.resolver_item,
                    file_name: item.file_name,
                    rel_path: item.rel_path,
                    size: item.size,
                })
                .collect(),
        }
    }
}

impl DaemonService {
    async fn rss_boolean_operation(
        &self,
        operation: ActorOperation,
    ) -> Result<Value, RpcErrorObject> {
        match self.actor.execute(operation).await {
            Ok(ActorResult::Boolean(true)) => Ok(json!({ "ok": true })),
            Ok(ActorResult::Boolean(false)) => Err(not_found()),
            Ok(_) => Err(internal_error("unexpected actor result".to_owned())),
            Err(error) => Err(actor_error(error)),
        }
    }
}

#[cfg(feature = "plugins")]
impl DaemonService {
    fn plugin_manager(
        &self,
    ) -> Result<&Arc<fluxdown_engine::plugin::PluginManager>, RpcErrorObject> {
        self.plugin_manager
            .as_ref()
            .ok_or_else(|| unsupported_error("plugin manager is unavailable"))
    }

    async fn list_plugins(&self) -> Result<Vec<fluxdown_protocol::PluginDto>, RpcErrorObject> {
        Ok(self
            .plugin_manager()?
            .list()
            .await
            .into_iter()
            .map(fluxdown_engine_protocol::plugin_info_to_dto)
            .collect())
    }

    async fn publish_plugins(&self) -> Result<(), RpcErrorObject> {
        let plugins = self.list_plugins().await?;
        self.events
            .publish(fluxdown_protocol::DaemonEvent::PluginsChanged(plugins));
        Ok(())
    }

    async fn market_client(&self) -> Result<fluxdown_engine::plugin::MarketClient, RpcErrorObject> {
        let config = self
            .db
            .get_all_config()
            .await
            .map_err(|error| internal_error(format!("{error:#}")))?;
        let sources = fluxdown_engine::plugin::MarketClient::source_config(&config);
        Ok(fluxdown_engine::plugin::MarketClient::new(
            self.plugin_manager()?.clone(),
            self.db.clone(),
            sources,
        ))
    }

    async fn plugin_missing_components(&self, identity: &str) -> Vec<String> {
        let Some(manager) = self.plugin_manager.as_ref() else {
            return Vec::new();
        };
        let permissions = manager.permissions_of(identity).await;
        fluxdown_engine::plugin::dependencies::missing_components(
            &self.db,
            &self.data_dir,
            &permissions,
        )
        .await
    }
}

#[cfg(feature = "components")]
impl DaemonService {
    async fn component_status(
        &self,
        component: fluxdown_protocol::ComponentKind,
    ) -> fluxdown_protocol::ComponentStatusDto {
        match component {
            fluxdown_protocol::ComponentKind::Ffmpeg => {
                fluxdown_protocol::ComponentStatusDto::Ffmpeg(
                    fluxdown_engine_protocol::ffmpeg_status_to_dto(
                        fluxdown_engine::components::ffmpeg_status(&self.db, &self.data_dir).await,
                    ),
                )
            }
            fluxdown_protocol::ComponentKind::Ytdlp => {
                fluxdown_protocol::ComponentStatusDto::Ytdlp(
                    fluxdown_engine_protocol::ytdlp_status_to_dto(
                        fluxdown_engine::components::ytdlp_status(&self.db, &self.data_dir).await,
                    ),
                )
            }
        }
    }

    async fn component_versions(
        &self,
        component: fluxdown_protocol::ComponentKind,
    ) -> Result<fluxdown_protocol::ComponentVersions, RpcErrorObject> {
        let config = self
            .db
            .get_all_config()
            .await
            .map_err(|error| internal_error(format!("{error:#}")))?;
        let proxy = fluxdown_engine::proxy_config::ProxyConfig::from_config_map(&config);
        let user_agent = config
            .get("global_user_agent")
            .map(String::as_str)
            .unwrap_or_default();
        let client = fluxdown_engine::downloader::build_client(&proxy, user_agent)
            .map_err(|error| internal_error(format!("{error:#}")))?;
        match component {
            fluxdown_protocol::ComponentKind::Ffmpeg => {
                fluxdown_engine::components::list_versions(&client)
                    .await
                    .map(fluxdown_engine_protocol::ffmpeg_versions_to_dto)
                    .map_err(|error| internal_error(error.to_string()))
            }
            fluxdown_protocol::ComponentKind::Ytdlp => {
                fluxdown_engine::components::list_ytdlp_versions(&client)
                    .await
                    .map(fluxdown_engine_protocol::ytdlp_versions_to_dto)
                    .map_err(|error| internal_error(error.to_string()))
            }
        }
    }

    async fn install_component(
        &self,
        component: fluxdown_protocol::ComponentKind,
        version: Option<String>,
    ) -> Result<Value, RpcErrorObject> {
        let flag = self.component_install_flag(component);
        if flag.swap(true, Ordering::SeqCst) {
            return Err(conflict_error("component install already in progress"));
        }
        let _guard = ComponentInstallGuard(flag);
        let config = self
            .db
            .get_all_config()
            .await
            .map_err(|error| internal_error(format!("{error:#}")))?;
        let proxy = fluxdown_engine::proxy_config::ProxyConfig::from_config_map(&config);
        let user_agent = config
            .get("global_user_agent")
            .map(String::as_str)
            .unwrap_or_default();
        let client = fluxdown_engine::downloader::build_client(&proxy, user_agent)
            .map_err(|error| internal_error(format!("{error:#}")))?;
        let component_name = component_name(component);
        let progress_events = self.events.clone();
        let progress = move |downloaded: u64, total: u64| {
            progress_events.publish(fluxdown_protocol::DaemonEvent::Engine(
                fluxdown_protocol::WsServerMsg::ComponentProgress {
                    component: component_name.to_owned(),
                    downloaded_bytes: i64::try_from(downloaded).unwrap_or(i64::MAX),
                    total_bytes: i64::try_from(total).unwrap_or(i64::MAX),
                },
            ));
        };
        let outcome = match component {
            fluxdown_protocol::ComponentKind::Ffmpeg => {
                fluxdown_engine::components::install_ffmpeg(
                    &self.db,
                    &self.data_dir,
                    &client,
                    version.as_deref(),
                    &progress,
                )
                .await
                .map(|_| ())
            }
            fluxdown_protocol::ComponentKind::Ytdlp => fluxdown_engine::components::install_ytdlp(
                &self.db,
                &self.data_dir,
                &client,
                version.as_deref(),
                &progress,
            )
            .await
            .map(|_| ()),
        };
        match outcome {
            Ok(()) => {
                self.events.publish(fluxdown_protocol::DaemonEvent::Engine(
                    fluxdown_protocol::WsServerMsg::ComponentResult {
                        component: component_name.to_owned(),
                        ok: true,
                        message: "installed".to_owned(),
                    },
                ));
                self.publish_component_statuses().await;
                Ok(json!({ "ok": true }))
            }
            Err(error) => {
                self.events.publish(fluxdown_protocol::DaemonEvent::Engine(
                    fluxdown_protocol::WsServerMsg::ComponentResult {
                        component: component_name.to_owned(),
                        ok: false,
                        message: error.to_string(),
                    },
                ));
                self.publish_component_statuses().await;
                Err(internal_error(error.to_string()))
            }
        }
    }

    async fn uninstall_component(
        &self,
        component: fluxdown_protocol::ComponentKind,
    ) -> Result<Value, RpcErrorObject> {
        if self
            .component_install_flag(component)
            .load(Ordering::SeqCst)
        {
            return Err(conflict_error("component install is in progress"));
        }
        match component {
            fluxdown_protocol::ComponentKind::Ffmpeg => {
                fluxdown_engine::components::uninstall_ffmpeg(&self.db, &self.data_dir).await
            }
            fluxdown_protocol::ComponentKind::Ytdlp => {
                fluxdown_engine::components::uninstall_ytdlp(&self.db, &self.data_dir).await
            }
        }
        .map_err(|error| internal_error(error.to_string()))?;
        self.publish_component_statuses().await;
        Ok(json!({ "ok": true }))
    }

    fn component_install_flag(&self, component: fluxdown_protocol::ComponentKind) -> &AtomicBool {
        match component {
            fluxdown_protocol::ComponentKind::Ffmpeg => &self.ffmpeg_installing,
            fluxdown_protocol::ComponentKind::Ytdlp => &self.ytdlp_installing,
        }
    }

    async fn publish_component_statuses(&self) {
        let statuses = vec![
            self.component_status(fluxdown_protocol::ComponentKind::Ffmpeg)
                .await,
            self.component_status(fluxdown_protocol::ComponentKind::Ytdlp)
                .await,
        ];
        self.events
            .publish(fluxdown_protocol::DaemonEvent::ComponentsChanged(statuses));
    }
}

#[cfg(feature = "components")]
struct ComponentInstallGuard<'a>(&'a AtomicBool);

#[cfg(feature = "components")]
impl Drop for ComponentInstallGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg(feature = "components")]
fn component_name(component: fluxdown_protocol::ComponentKind) -> &'static str {
    match component {
        fluxdown_protocol::ComponentKind::Ffmpeg => "ffmpeg",
        fluxdown_protocol::ComponentKind::Ytdlp => "ytdlp",
    }
}

impl DaemonService {
    async fn runtime_stats(&self) -> fluxdown_protocol::DaemonRuntimeStatsDto {
        let snapshot = self.daemon_snapshot();
        let save_dir = snapshot
            .config
            .values
            .get("default_save_dir")
            .cloned()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or_else(fluxdown_engine::user_dirs::download_dir_or_cwd);
        fluxdown_protocol::DaemonRuntimeStatsDto {
            active_tasks: u32::try_from(
                snapshot
                    .tasks
                    .iter()
                    .filter(|task| matches!(task.status, 1 | 5))
                    .count(),
            )
            .unwrap_or(u32::MAX),
            pending_tasks: u32::try_from(
                snapshot
                    .tasks
                    .iter()
                    .filter(|task| task.status == 0)
                    .count(),
            )
            .unwrap_or(u32::MAX),
            total_download_bps: snapshot.runtime_stats.total_download_bps,
            total_upload_bps: snapshot.runtime_stats.total_upload_bps,
            disk_free_bytes: fluxdown_engine::disk_space::available_space_checked(
                std::path::PathBuf::from(&save_dir),
            )
            .await,
            save_dir,
        }
    }

    async fn list_directories(
        &self,
        requested_path: String,
    ) -> Result<fluxdown_protocol::FsListResponse, RpcErrorObject> {
        let snapshot = self.daemon_snapshot();
        let base = if requested_path.trim().is_empty() {
            let configured = snapshot
                .config
                .values
                .get("default_save_dir")
                .cloned()
                .unwrap_or_default();
            if configured.trim().is_empty() {
                snapshot.runtime_stats.save_dir
            } else {
                configured
            }
        } else {
            requested_path
        };
        let base_path = std::path::Path::new(&base);
        let parent = base_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(|path| path.to_string_lossy().into_owned());
        let mut dirs = Vec::new();
        if let Ok(mut entries) = tokio::fs::read_dir(base_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if !file_type.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                dirs.push(fluxdown_protocol::FsEntry {
                    name,
                    path: entry.path().to_string_lossy().into_owned(),
                });
            }
        }
        dirs.sort_by_key(|entry| entry.name.to_lowercase());
        Ok(fluxdown_protocol::FsListResponse {
            path: base,
            parent,
            dirs,
        })
    }
}

#[cfg(feature = "plugins")]
fn manifest_item_to_preview_dto(
    item: fluxdown_engine::model::ManifestItemInfo,
) -> fluxdown_protocol::PreviewItemDto {
    fluxdown_protocol::PreviewItemDto {
        id: item.id,
        name: item.name,
        path: item.path,
        size: item.size,
        variants: item
            .variants
            .into_iter()
            .map(|variant| fluxdown_protocol::PreviewVariantDto {
                id: variant.id,
                label: variant.label,
                size: variant.size,
            })
            .collect(),
    }
}

#[derive(Default, Deserialize)]
struct FsListParams {
    #[serde(default)]
    path: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RssSourceIdParams {
    source_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RssItemActionParams {
    source_id: String,
    #[serde(default)]
    guid: String,
    action: String,
}

#[cfg(feature = "plugins")]
#[derive(Deserialize)]
struct PluginIdentityParams {
    identity: String,
}

#[cfg(feature = "plugins")]
#[derive(Deserialize)]
struct PluginEnabledParams {
    identity: String,
    enabled: bool,
}

#[cfg(feature = "plugins")]
#[derive(Deserialize)]
struct PluginSettingsParams {
    identity: String,
    entries: HashMap<String, String>,
}

#[cfg(feature = "plugins")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginInstallParams {
    blob_id: String,
}

#[cfg(feature = "plugins")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskIdParams {
    task_id: String,
}

#[derive(Deserialize)]
struct IdParams {
    #[serde(alias = "taskId", alias = "queueId", alias = "groupId")]
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameParams {
    task_id: String,
    file_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteParams {
    #[serde(alias = "taskId", alias = "groupId")]
    id: String,
    #[serde(default)]
    delete_files: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedLimitsParams {
    task_id: String,
    ratio_limit_milli: i64,
    post_ratio_limit_milli: i64,
    seed_time_limit_minutes: i64,
    inactive_time_limit_minutes: i64,
    upload_limit_bps: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateQueueParams {
    queue_id: String,
    #[serde(flatten)]
    request: CreateQueueRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueScheduleParams {
    queue_id: String,
    enabled: bool,
    #[serde(default)]
    start_time: String,
    #[serde(default)]
    stop_time: String,
    #[serde(default)]
    days: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueOrderParams {
    queue_id: String,
    task_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveTaskParams {
    task_id: String,
    #[serde(default)]
    queue_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProxyTestParams {
    proxy_type: String,
    host: String,
    port: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

enum QueueAction {
    Delete,
    Start,
    Stop,
}
enum GroupAction {
    Pause,
    Resume,
}

fn parse_params<T: DeserializeOwned>(params: Option<Value>) -> Result<T, RpcErrorObject> {
    let Some(params) = params else {
        return Err(invalid_argument("params", "params are required"));
    };
    serde_json::from_value(params).map_err(|error| invalid_argument("params", &error.to_string()))
}

fn parse_optional_params<T: DeserializeOwned + Default>(
    params: Option<Value>,
) -> Result<T, RpcErrorObject> {
    params.map_or_else(
        || Ok(T::default()),
        |value| {
            serde_json::from_value(value)
                .map_err(|error| invalid_argument("params", &error.to_string()))
        },
    )
}

fn to_value(value: impl serde::Serialize) -> Result<Value, RpcErrorObject> {
    serde_json::to_value(value).map_err(|error| internal_error(error.to_string()))
}

fn actor_error(error: ActorCallError) -> RpcErrorObject {
    match error {
        ActorCallError::Unavailable => RpcErrorObject::application(
            error.to_string(),
            RpcErrorData::new(ApplicationErrorCode::Unavailable, true),
        ),
        ActorCallError::Operation(ActorError::InvalidArgument(message)) => {
            invalid_argument("params", &message)
        }
        ActorCallError::Operation(ActorError::Operation(message)) => internal_error(message),
        ActorCallError::Operation(ActorError::RevisionConflict { current }) => {
            RpcErrorObject::application(
                "config revision conflict",
                RpcErrorData {
                    code: ApplicationErrorCode::Conflict,
                    retryable: false,
                    field: None,
                    revision: Some(current),
                },
            )
        }
        ActorCallError::Operation(ActorError::NotFound) => not_found(),
    }
}

fn require_local_agent(is_local_agent: bool) -> Result<(), RpcErrorObject> {
    if is_local_agent {
        Ok(())
    } else {
        Err(RpcErrorObject::application(
            "migration methods are restricted to the local agent",
            RpcErrorData::new(ApplicationErrorCode::Unauthorized, false),
        ))
    }
}

fn invalid_argument(field: &str, message: &str) -> RpcErrorObject {
    RpcErrorObject::application(
        message,
        RpcErrorData {
            code: ApplicationErrorCode::InvalidArgument,
            retryable: false,
            field: Some(field.to_owned()),
            revision: None,
        },
    )
}

fn not_found() -> RpcErrorObject {
    RpcErrorObject::application(
        "not found",
        RpcErrorData::new(ApplicationErrorCode::NotFound, false),
    )
}

fn internal_error(message: String) -> RpcErrorObject {
    RpcErrorObject::application(
        message,
        RpcErrorData::new(ApplicationErrorCode::Internal, false),
    )
}

#[cfg(feature = "components")]
fn conflict_error(message: &str) -> RpcErrorObject {
    RpcErrorObject::application(
        message,
        RpcErrorData::new(ApplicationErrorCode::Conflict, false),
    )
}

fn unsupported_error(message: &str) -> RpcErrorObject {
    RpcErrorObject::application(
        message,
        RpcErrorData::new(ApplicationErrorCode::Unsupported, false),
    )
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{ApplicationErrorCode, RpcErrorData};

    use super::DaemonService;

    #[tokio::test]
    async fn every_canonical_daemon_method_reaches_a_real_dispatch_branch() {
        let dir = std::env::temp_dir().join(format!(
            "fluxdown_daemon_dispatch_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        tokio::fs::create_dir_all(&dir)
            .await
            .expect("create daemon dispatch dir");
        let db = fluxdown_engine::db::Db::open(&dir)
            .await
            .expect("open daemon dispatch db");
        db.init_default_config("/tmp")
            .await
            .expect("seed daemon dispatch config");
        let events =
            crate::event_hub::DaemonEventHub::new(fluxdown_protocol::DaemonSnapshot::default(), 32);
        let selections = crate::selection::DaemonSelection::new(events.clone());
        let blobs = std::sync::Arc::new(
            crate::blob_store::BlobStore::open(dir.join("blobs"))
                .await
                .expect("open daemon dispatch blobs"),
        );
        let service = DaemonService::new(
            crate::service_hello("dispatch-test", Vec::new()),
            events,
            selections,
            blobs,
            crate::actor::DaemonActorHandle::disconnected(),
            db,
            #[cfg(any(feature = "plugins", feature = "components"))]
            dir.clone(),
            #[cfg(feature = "plugins")]
            None,
        );

        for method_name in fluxdown_protocol::method::ALL_METHODS
            .iter()
            .copied()
            .filter(|name| name.starts_with("daemon."))
        {
            let result = service
                .call_inner(
                    "dispatch-test",
                    true,
                    method_name,
                    Some(serde_json::json!({})),
                )
                .await;
            if let Err(error) = result
                && error.data.as_ref().map(|data| data.code)
                    == Some(ApplicationErrorCode::Unsupported)
            {
                assert!(
                    optional_feature_method(method_name),
                    "{method_name} fell through daemon dispatch: {:?}",
                    error.data.unwrap_or_else(|| {
                        RpcErrorData::new(ApplicationErrorCode::Internal, false)
                    })
                );
            }
        }
        drop(service);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    fn optional_feature_method(method_name: &str) -> bool {
        method_name == fluxdown_protocol::method::DAEMON_GROUP_RESOLVE_PREVIEW
            || method_name.starts_with("daemon.plugin.")
            || method_name.starts_with("daemon.component.")
    }
}
