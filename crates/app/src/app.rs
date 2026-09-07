use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    env,
    rc::Rc,
    sync::Arc,
};

use fluxdown_ui_account::AccountView;
use fluxdown_ui_downloads::{
    DOWNLOAD_ICON_PATH, DownloadView, NewDownloadContext, NewDownloadView,
};
use fluxdown_ui_extensions::ExtensionsView;
use fluxdown_ui_i18n::{I18nCatalog, I18nError, Translator, keys};
use fluxdown_ui_rss::RssView;
use fluxdown_ui_settings::{SettingsContentSlots, SettingsStore, SettingsView, component_locale};
use fluxdown_ui_shell::{
    AuxiliaryWindowView, RouteId, ShellAction, ShellRoute, ShellView, auxiliary_window_options,
    main_window_options,
};
use gpui::{
    App, AppContext as _, Bounds, Entity, WeakEntity, Window, WindowBounds, WindowHandle, px, size,
};
use gpui_component::{Icon, IconName, Root};

use crate::account_port::AgentAccountPort;
use crate::agent_client::{AgentClient, AgentClientConfig, AgentClientEvent};
use crate::assets::DesktopAssets;
use crate::capability_ports::{AgentExtensionsPort, AgentRssPort};
use crate::downloads_port::AgentDownloadsPort;
use crate::launch::{self, LaunchOptions};
use crate::service_bootstrap::ServiceBootstrap;
use crate::settings_port::AgentSettingsPort;

const MI_SANS_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Regular.ttf");
const MI_SANS_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Medium.ttf");
const MI_SANS_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Semibold.ttf");

struct ClientProjection {
    last_snapshot: Option<fluxdown_protocol::AgentSnapshot>,
    account: Option<WeakEntity<AccountView>>,
    rss: Option<WeakEntity<RssView>>,
    extensions: Option<WeakEntity<ExtensionsView>>,
}
const SETTINGS_WINDOW_SIZE: gpui::Size<gpui::Pixels> = size(px(1240.), px(760.));
const NEW_DOWNLOAD_WINDOW_SIZE: gpui::Size<gpui::Pixels> = size(px(640.), px(760.));
const NEW_DOWNLOAD_WINDOW_MIN_SIZE: gpui::Size<gpui::Pixels> = size(px(560.), px(600.));
pub(crate) fn run() -> Result<(), I18nError> {
    let launch = LaunchOptions::from_args(env::args().skip(1));
    let token_path = agent_token_path();
    let instance_dir = launch::instance_dir(&token_path);
    let instance_lock = match launch::InstanceLock::try_acquire(&instance_dir) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => None,
        Err(error) => {
            eprintln!("FluxDown desktop instance lock unavailable: {error:#}");
            None
        }
    };
    let agent_config = AgentClientConfig {
        rpc_url: env::var("FLUXDOWN_AGENT_URL")
            .unwrap_or_else(|_| "ws://127.0.0.1:17800/rpc".to_owned()),
        bearer_path: token_path,
    };
    if instance_lock.is_none() {
        // 已有实例：把链接交给 agent 后直接退出，不再开第二个窗口。
        forward_urls_and_exit(&agent_config, &launch.urls, &launch.torrent_files);
        return Ok(());
    }
    let _instance_lock = instance_lock;

    let catalog = Arc::new(I18nCatalog::load_embedded()?);
    let translator = catalog.translator(&system_locale());
    let locale = component_locale(translator.locale()).to_owned();

    let bootstrap = Arc::new(ServiceBootstrap::new());
    let (agent_client, mut agent_events) = match AgentClient::start(agent_config, bootstrap) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("failed to start FluxDown agent client: {error:#}");
            return Ok(());
        }
    };
    submit_captures_detached(
        &agent_client,
        launch.urls.clone(),
        launch.torrent_files.clone(),
    );
    let open_urls_client = agent_client.clone();

    let application = gpui_platform::application().with_assets(DesktopAssets);
    application.on_open_urls(move |urls| {
        let files = urls
            .iter()
            .filter_map(|url| launch::torrent_path(url))
            .collect();
        let urls = urls
            .into_iter()
            .filter(|url| launch::is_capture_url(url))
            .collect();
        submit_captures_detached(&open_urls_client, urls, files);
    });
    application.run(move |cx| {
        if let Err(error) = cx.text_system().add_fonts(vec![
            Cow::Borrowed(MI_SANS_REGULAR),
            Cow::Borrowed(MI_SANS_MEDIUM),
            Cow::Borrowed(MI_SANS_SEMIBOLD),
        ]) {
            eprintln!("failed to load FluxDown UI fonts: {error:#}");
            return;
        }

        gpui_component::init(cx);
        fluxdown_ui_theme::init(cx);
        gpui_component::set_locale(&locale);
        let translator = cx.new(|_| translator);
        let bounds = Bounds::centered(None, size(px(1120.), px(760.)), cx);
        let mut options = main_window_options();
        options.window_bounds = Some(WindowBounds::Windowed(bounds));

        // 设置存储跨窗口存活：窗口关闭后防抖中的写回仍完成，快照/事件持续进入。
        let settings_store =
            cx.new(|_| SettingsStore::new(Arc::new(AgentSettingsPort::new(agent_client.clone()))));
        let quit_store = settings_store.clone();
        cx.on_app_quit(move |cx| {
            let calls = quit_store.update(cx, |store, _| store.drain_pending_calls());
            async move {
                for call in calls {
                    let _ = call.await;
                }
            }
        })
        .detach();
        let client_projection = Rc::new(RefCell::new(ClientProjection {
            last_snapshot: None,
            account: None,
            rss: None,
            extensions: None,
        }));
        let settings_window = Rc::new(Cell::new(None));
        let start_minimized = launch.minimized;
        if let Err(error) = cx.open_window(options, move |window, cx| {
            let downloads_port = Arc::new(AgentDownloadsPort::new(agent_client.clone()));
            let downloads =
                cx.new(|cx| DownloadView::new(translator.clone(), downloads_port, window, cx));
            let new_download_window = Rc::new(Cell::new(None));
            let new_download_translator = translator.clone();
            let new_download_target = downloads.downgrade();
            downloads.update(cx, |downloads, _| {
                downloads.set_new_download_opener(Rc::new(move |context, window, cx| {
                    show_new_download_window(
                        new_download_translator.clone(),
                        new_download_target.clone(),
                        context,
                        new_download_window.as_ref(),
                        window,
                        cx,
                    );
                }));
            });
            let rss_port = Arc::new(AgentRssPort::new(agent_client.clone()));
            let rss = cx.new(|cx| RssView::new(translator.clone(), rss_port, window, cx));
            client_projection.borrow_mut().rss = Some(rss.downgrade());
            let downloads_events = downloads.downgrade();
            let projection_events = Rc::clone(&client_projection);
            let prefs_translator = translator.clone();
            let settings_store_events = settings_store.clone();
            cx.spawn(async move |cx| {
                while let Some(event) = agent_events.recv().await {
                    match &event {
                        AgentClientEvent::Snapshot(snapshot) => {
                            projection_events.borrow_mut().last_snapshot =
                                Some(snapshot.as_ref().clone());
                            let values = snapshot.preferences.values.clone();
                            cx.update(|cx| {
                                apply_preferences(&values, &prefs_translator, cx);
                            });
                        }
                        AgentClientEvent::Event(frame) => {
                            if let fluxdown_protocol::ServiceEvent::Agent(
                                fluxdown_protocol::AgentEvent::PreferencesChanged(prefs),
                            ) = &frame.event
                            {
                                let values = prefs.values.clone();
                                cx.update(|cx| {
                                    apply_preferences(&values, &prefs_translator, cx);
                                });
                            }
                        }
                        _ => {}
                    }
                    if downloads_events
                        .update(cx, |downloads, cx| match &event {
                            AgentClientEvent::Snapshot(snapshot) => {
                                downloads.replace_snapshot(snapshot, cx);
                            }
                            AgentClientEvent::Event(frame) => {
                                downloads.apply_event(&frame.event, cx);
                            }
                            AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                downloads.mark_stale(cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                    settings_store_events.update(cx, |store, cx| match &event {
                        AgentClientEvent::Snapshot(snapshot) => {
                            store.replace_snapshot(snapshot, cx);
                        }
                        AgentClientEvent::Event(frame) => {
                            store.apply_event(&frame.event, cx);
                        }
                        AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                            store.mark_stale(cx);
                        }
                    });
                    let account = projection_events.borrow().account.clone();
                    if let Some(account) = account {
                        let _ = account.update(cx, |account, cx| match &event {
                            AgentClientEvent::Snapshot(snapshot) => {
                                account.replace_snapshot(snapshot, cx);
                            }
                            AgentClientEvent::Event(frame) => {
                                account.apply_event(&frame.event, cx);
                            }
                            AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                account.mark_stale(cx);
                            }
                        });
                    }
                    let rss = projection_events.borrow().rss.clone();
                    if let Some(rss) = rss {
                        let _ = rss.update(cx, |rss, cx| match &event {
                            AgentClientEvent::Snapshot(snapshot) => {
                                rss.replace_snapshot(snapshot, cx);
                            }
                            AgentClientEvent::Event(frame) => {
                                rss.apply_event(&frame.event, cx);
                            }
                            AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                rss.mark_stale(cx);
                            }
                        });
                    }
                    let extensions = projection_events.borrow().extensions.clone();
                    if let Some(extensions) = extensions {
                        let _ = extensions.update(cx, |extensions, cx| match &event {
                            AgentClientEvent::Snapshot(snapshot) => {
                                extensions.replace_snapshot(snapshot, cx);
                            }
                            AgentClientEvent::Event(frame) => {
                                extensions.apply_event(&frame.event, cx);
                            }
                            AgentClientEvent::Stale | AgentClientEvent::Fatal(_) => {
                                extensions.mark_stale(cx);
                            }
                        });
                    }
                    if let AgentClientEvent::Fatal(error) = &event {
                        eprintln!("fatal FluxDown agent error: {:?}", error.code);
                    }
                }
            })
            .detach();
            let routes = vec![
                ShellRoute::new(
                    RouteId::new("downloads"),
                    "activity-downloads",
                    "activity-downloads-tooltip",
                    keys::MOBILE_NAV_DOWNLOADS,
                    Icon::empty().path(DOWNLOAD_ICON_PATH),
                    downloads.clone().into(),
                ),
                ShellRoute::new(
                    RouteId::new("rss"),
                    "activity-rss",
                    "activity-rss-tooltip",
                    "rssAddSource",
                    Icon::new(IconName::Globe),
                    rss.clone().into(),
                ),
            ];
            let settings_translator = translator.clone();
            let settings_window = Rc::clone(&settings_window);
            let settings_client = agent_client.clone();
            let settings_store_window = settings_store.clone();
            let settings_projection = Rc::clone(&client_projection);
            let actions = vec![ShellAction::new(
                "activity-settings",
                "activity-settings-tooltip",
                keys::SETTINGS,
                Icon::new(IconName::Settings),
                move |window, cx| {
                    show_settings_window(
                        settings_translator.clone(),
                        settings_client.clone(),
                        settings_store_window.clone(),
                        Rc::clone(&settings_projection),
                        settings_window.as_ref(),
                        window,
                        cx,
                    );
                },
            )];
            let shell = cx.new(|cx| ShellView::new(translator, routes, actions, cx));
            if start_minimized {
                window.minimize_window();
            }
            cx.new(|cx| Root::new(shell, window, cx))
        }) {
            eprintln!("failed to open FluxDown desktop window: {error:#}");
            return;
        }

        cx.activate(true);
    });

    Ok(())
}

/// 偏好快照 → 全局外观与语言。每次快照/偏好事件都幂等应用。
fn apply_preferences(
    values: &std::collections::BTreeMap<String, serde_json::Value>,
    translator: &Entity<Translator>,
    cx: &mut App,
) {
    fluxdown_ui_theme::apply_appearance_preferences(values, cx);
    if let Some(locale) = values
        .get("general.locale")
        .and_then(serde_json::Value::as_str)
    {
        let target = if locale == "system" {
            system_locale()
        } else {
            locale.to_owned()
        };
        translator.update(cx, |translator, cx| {
            if translator.set_locale(&target) {
                gpui_component::set_locale(component_locale(translator.locale()));
                cx.notify();
            }
        });
    }
}

/// 外部链接 / `.torrent` 文件 → agent 捕获入口的 RPC 列表。
fn capture_calls(
    client: &Arc<AgentClient>,
    urls: Vec<String>,
    files: Vec<std::path::PathBuf>,
) -> Vec<(String, crate::agent_client::AgentFuture<serde_json::Value>)> {
    let mut calls = Vec::with_capacity(urls.len() + files.len());
    for url in urls {
        let url = launch::normalize_capture_url(&url);
        let future = client.call::<serde_json::Value, serde_json::Value>(
            fluxdown_protocol::method::AGENT_CAPTURE_SUBMIT,
            Some(serde_json::json!({ "request": { "url": url }, "silent": true })),
        );
        calls.push((url, future));
    }
    for file in files {
        let path = file.display().to_string();
        let future = client.call::<serde_json::Value, serde_json::Value>(
            fluxdown_protocol::method::AGENT_CAPTURE_SUBMIT_TORRENT_FILE,
            Some(serde_json::json!({ "path": path, "silent": true })),
        );
        calls.push((path, future));
    }
    calls
}

/// 主实例：不阻塞 UI 线程，在后台把链接交给 agent。
fn submit_captures_detached(
    client: &Arc<AgentClient>,
    urls: Vec<String>,
    files: Vec<std::path::PathBuf>,
) {
    if urls.is_empty() && files.is_empty() {
        return;
    }
    let calls = capture_calls(client, urls, files);
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        for (source, future) in calls {
            if let Err(error) = runtime.block_on(future) {
                eprintln!("failed to submit {source}: {:?}", error.code);
            }
        }
    });
}

/// 次实例：不开窗口，只把链接交给 agent 后退出。
fn forward_urls_and_exit(
    config: &AgentClientConfig,
    urls: &[String],
    files: &[std::path::PathBuf],
) {
    if urls.is_empty() && files.is_empty() {
        return;
    }
    let bootstrap = Arc::new(ServiceBootstrap::new());
    let Ok((client, _events)) = AgentClient::start(config.clone(), bootstrap) else {
        eprintln!("failed to reach the running FluxDown instance");
        return;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    for (source, future) in capture_calls(&client, urls.to_vec(), files.to_vec()) {
        if let Err(error) = runtime.block_on(future) {
            eprintln!("failed to forward {source}: {:?}", error.code);
        }
    }
}

fn show_settings_window(
    translator: Entity<Translator>,
    agent_client: Arc<AgentClient>,
    settings_store: Entity<SettingsStore>,
    projection: Rc<RefCell<ClientProjection>>,
    settings_window: &Cell<Option<WindowHandle<Root>>>,
    parent_window: &mut Window,
    cx: &mut App,
) {
    if let Some(handle) = settings_window.get() {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        settings_window.set(None);
    }

    let display_id = parent_window.display(cx).map(|display| display.id());
    let title = translator.read(cx).text(keys::SETTINGS).to_owned();
    let bounds = Bounds::centered(display_id, SETTINGS_WINDOW_SIZE, cx);
    let mut options = auxiliary_window_options(title.clone());
    options.display_id = display_id;
    options.window_bounds = Some(WindowBounds::Windowed(bounds));
    options.window_min_size = Some(size(px(1000.), px(600.)));

    match cx.open_window(options, move |window, cx| {
        let window_translator = translator.clone();
        let account_port = Arc::new(AgentAccountPort::new(agent_client.clone()));
        let extensions_port = Arc::new(AgentExtensionsPort::new(agent_client));
        let account = cx.new(|cx| AccountView::new(translator.clone(), account_port, window, cx));
        let extensions = cx.new(|cx| ExtensionsView::new(translator.clone(), extensions_port, cx));
        let settings = cx.new(|cx| {
            SettingsView::new(
                translator,
                settings_store,
                SettingsContentSlots {
                    account: Some(account.clone().into()),
                    extensions: Some(extensions.clone().into()),
                },
                cx,
            )
        });
        if let Some(snapshot) = projection.borrow().last_snapshot.as_ref() {
            account.update(cx, |account, cx| {
                account.replace_snapshot(snapshot, cx);
            });
            extensions.update(cx, |extensions, cx| {
                extensions.replace_snapshot(snapshot, cx);
            });
        }
        let mut projection = projection.borrow_mut();
        projection.account = Some(account.downgrade());
        projection.extensions = Some(extensions.downgrade());
        drop(projection);
        let window_view = cx.new(|cx| {
            AuxiliaryWindowView::new(
                window_translator,
                keys::SETTINGS,
                settings.clone().into(),
                cx,
            )
        });
        cx.new(|cx| Root::new(window_view, window, cx))
    }) {
        Ok(handle) => settings_window.set(Some(handle)),
        Err(error) => eprintln!("failed to open FluxDown settings window: {error:#}"),
    }
}

fn show_new_download_window(
    translator: Entity<Translator>,
    downloads: WeakEntity<DownloadView>,
    context: NewDownloadContext,
    new_download_window: &Cell<Option<WindowHandle<Root>>>,
    parent_window: &mut Window,
    cx: &mut App,
) {
    if let Some(handle) = new_download_window.get() {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        new_download_window.set(None);
    }

    let display_id = parent_window.display(cx).map(|display| display.id());
    let title = translator.read(cx).text(keys::NEW_DOWNLOAD).to_owned();
    let bounds = Bounds::centered(display_id, NEW_DOWNLOAD_WINDOW_SIZE, cx);
    let mut options = auxiliary_window_options(title);
    options.display_id = display_id;
    options.window_bounds = Some(WindowBounds::Windowed(bounds));
    options.window_min_size = Some(NEW_DOWNLOAD_WINDOW_MIN_SIZE);
    options.is_resizable = true;
    match cx.open_window(options, move |window, cx| {
        let on_submit = Rc::new(move |submission, _: &mut Window, cx: &mut App| {
            let _ = downloads.update(cx, |downloads, cx| {
                downloads.create_download(submission, cx);
            });
        });
        let form = cx.new(|cx| {
            NewDownloadView::new(translator.clone(), context.clone(), on_submit, window, cx)
        });
        let window_view =
            cx.new(|cx| AuxiliaryWindowView::new(translator, keys::NEW_DOWNLOAD, form.into(), cx));
        cx.new(|cx| Root::new(window_view, window, cx))
    }) {
        Ok(handle) => new_download_window.set(Some(handle)),
        Err(error) => eprintln!("failed to open FluxDown new download window: {error:#}"),
    }
}

fn agent_token_path() -> std::path::PathBuf {
    if let Some(path) = env::var_os("FLUXDOWN_AGENT_TOKEN_FILE") {
        return path.into();
    }
    if let Some(path) = env::var_os("FLUXDOWN_AGENT_DATA_DIR") {
        return std::path::PathBuf::from(path).join("agent.token");
    }
    directories::ProjectDirs::from("dev", "zerx", "FluxDown")
        .map(|project| project.data_dir().join("agent").join("agent.token"))
        .unwrap_or_else(|| std::path::PathBuf::from("agent.token"))
}

fn system_locale() -> String {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(locale) = env::var(key)
            && !locale.trim().is_empty()
        {
            return locale;
        }
    }
    "en".to_owned()
}
