//! 仅发送安装一次与每日活跃两类匿名部署事件。

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::state::{AgentState, StateStore};

const BAKED_APP_KEY: &str = match option_env!("FLUXDOWN_ANALYTICS_APP_KEY") {
    Some(value) => value,
    None => "",
};
const DEFAULT_ENDPOINT: &str =
    "https://ops.zerx.dev/api/zerx.v1.AnalyticsIngestService/TrackEvents";

pub struct AnalyticsWorker {
    state: Arc<Mutex<AgentState>>,
    store: Arc<StateStore>,
    client: reqwest::Client,
    endpoint: String,
    app_key: String,
}

impl AnalyticsWorker {
    pub fn new(
        state: Arc<Mutex<AgentState>>,
        store: Arc<StateStore>,
    ) -> Result<Self, reqwest::Error> {
        let endpoint = std::env::var("FLUXDOWN_ANALYTICS_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned());
        let app_key = std::env::var("FLUXDOWN_ANALYTICS_APP_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| BAKED_APP_KEY.to_owned());
        Ok(Self {
            state,
            store,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()?,
            endpoint,
            app_key,
        })
    }

    pub async fn run(self, cancel: CancellationToken) {
        if analytics_disabled_by_env() || self.app_key.trim().is_empty() {
            return;
        }
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_secs(10)) => {},
        }
        loop {
            self.report_once().await;
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(3600)) => {},
            }
        }
    }

    async fn report_once(&self) {
        let (enabled, device_id, installed, last_day) = {
            let state = self.state.lock().await;
            let enabled = state
                .preferences
                .values
                .get("analytics_enabled")
                .or_else(|| state.preferences.values.get("general.analytics_enabled"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            (
                enabled,
                state.device_id.clone(),
                state.analytics_install_reported,
                state.analytics_last_active_day,
            )
        };
        if !enabled || device_id.is_empty() {
            return;
        }
        let mut install_reported = installed;
        if !installed && self.track("app_installed", &device_id).await {
            install_reported = true;
        }
        let today = epoch_days();
        let active_reported = last_day == today || self.track("app_active", &device_id).await;
        if install_reported != installed || (active_reported && last_day != today) {
            let mut state = self.state.lock().await;
            state.analytics_install_reported = install_reported;
            if active_reported {
                state.analytics_last_active_day = today;
            }
            if let Err(error) = self.store.save(&state).await {
                tracing::warn!(error = %error, "persisting analytics markers failed");
            }
        }
    }

    async fn track(&self, event_name: &str, device_id: &str) -> bool {
        let payload = serde_json::json!({
            "events": [{
                "sessionId": device_id,
                "eventName": event_name,
                "systemProps": {
                    "osName": os_name(),
                    "osVersion": std::env::consts::ARCH,
                    "appVersion": env!("CARGO_PKG_VERSION"),
                    "locale": "",
                    "isDebug": cfg!(debug_assertions),
                },
                "props": {"edition": "desktop-agent"},
            }]
        });
        match self
            .client
            .post(&self.endpoint)
            .header("App-Key", &self.app_key)
            .json(&payload)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => true,
            Ok(response) => {
                tracing::debug!(status = %response.status(), event_name, "analytics event rejected");
                false
            }
            Err(error) => {
                tracing::debug!(error = %error, event_name, "analytics event failed");
                false
            }
        }
    }
}

fn analytics_disabled_by_env() -> bool {
    matches!(
        std::env::var("FLUXDOWN_ANALYTICS").as_deref(),
        Ok("off") | Ok("0") | Ok("false")
    )
}

fn epoch_days() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() / 86_400)
        .unwrap_or(0)
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Linux",
        "windows" => "Windows",
        "macos" => "macOS",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::epoch_days;

    #[test]
    fn epoch_day_is_stable_within_process() {
        assert_eq!(epoch_days(), epoch_days());
    }
}
