use std::sync::Arc;

use fluxdown_ui_settings::{PortFuture, SettingsPort};

use crate::agent_client::AgentClient;

/// 设置能力的 agent 适配器：方法名与 JSON 直通单一会话。
pub struct AgentSettingsPort {
    client: Arc<AgentClient>,
}

impl AgentSettingsPort {
    #[must_use]
    pub fn new(client: Arc<AgentClient>) -> Self {
        Self { client }
    }
}

impl SettingsPort for AgentSettingsPort {
    fn call(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> PortFuture<serde_json::Value> {
        self.client.call(method, Some(params))
    }
}
