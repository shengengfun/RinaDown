use std::sync::Arc;

use fluxdown_ui_account::{AccountCommand, AccountPort, PortFuture};

use crate::agent_client::AgentClient;

pub struct AgentAccountPort {
    client: Arc<AgentClient>,
}

impl AgentAccountPort {
    #[must_use]
    pub fn new(client: Arc<AgentClient>) -> Self {
        Self { client }
    }
}

impl AccountPort for AgentAccountPort {
    fn execute(&self, command: AccountCommand) -> PortFuture<serde_json::Value> {
        let client = self.client.clone();
        let (method, params) = match command {
            AccountCommand::Auth { method, params }
            | AccountCommand::Profile { method, params }
            | AccountCommand::Device { method, params }
            | AccountCommand::Plan { method, params }
            | AccountCommand::Order { method, params }
            | AccountCommand::Referral { method, params } => (method, params),
        };
        client.call(method, Some(params))
    }
}
