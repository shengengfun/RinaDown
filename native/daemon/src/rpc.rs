//! WebSocket JSON-RPC 会话状态与首帧握手门禁。

use std::sync::Arc;

use fluxdown_protocol::{
    ApplicationErrorCode, RpcErrorData, RpcErrorObject, RpcRequest, RpcResponse, ServiceRole,
    validate_first_request,
};
use uuid::Uuid;

use crate::service::DaemonService;

/// 单次入站请求的响应及会话状态变化。
pub struct SessionReply {
    pub response: RpcResponse,
    pub became_ready: bool,
}

/// 单条 WebSocket 连接的握手状态。
pub struct RpcSession {
    connection_id: String,
    ready: bool,
    is_local_agent: bool,
    service: Arc<DaemonService>,
}

impl RpcSession {
    #[must_use]
    pub fn new(service: Arc<DaemonService>) -> Self {
        Self {
            connection_id: Uuid::new_v4().to_string(),
            ready: false,
            is_local_agent: false,
            service,
        }
    }

    #[must_use]
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// 解析一条文本帧。首帧只能是兼容的 `system.hello`。
    pub async fn handle_text(&mut self, text: &str) -> SessionReply {
        let request = match serde_json::from_str::<RpcRequest>(text) {
            Ok(request) => request,
            Err(error) => {
                return SessionReply {
                    response: RpcResponse::parse_failure(error.to_string()),
                    became_ready: false,
                };
            }
        };
        if !self.ready {
            return self.handle_hello(request);
        }
        let id = request.id.clone();
        if let Err(data) = request.validate() {
            return SessionReply {
                response: RpcResponse::failure(
                    id,
                    RpcErrorObject::application("invalid JSON-RPC version", data),
                ),
                became_ready: false,
            };
        }
        if request.method == fluxdown_protocol::method::SYSTEM_HELLO {
            return SessionReply {
                response: RpcResponse::failure(
                    id,
                    RpcErrorObject::application(
                        "system.hello is only valid as the first frame",
                        RpcErrorData::new(ApplicationErrorCode::Conflict, false),
                    ),
                ),
                became_ready: false,
            };
        }
        SessionReply {
            response: self
                .service
                .call(&self.connection_id, self.is_local_agent, request)
                .await,
            became_ready: false,
        }
    }

    /// 连接断开时释放连接所有权选择订阅。
    pub fn disconnect(&self) {
        self.service.selections().unsubscribe(&self.connection_id);
    }

    fn handle_hello(&mut self, request: RpcRequest) -> SessionReply {
        let id = request.id.clone();
        match validate_first_request(&request, ServiceRole::Daemon) {
            Ok(hello) => match serde_json::to_value(self.service.hello()) {
                Ok(result) => {
                    self.ready = true;
                    self.is_local_agent = hello.client_name == "fluxdown-agent";
                    SessionReply {
                        response: RpcResponse::success(id, result),
                        became_ready: true,
                    }
                }
                Err(error) => SessionReply {
                    response: RpcResponse::failure(
                        id,
                        RpcErrorObject::application(
                            error.to_string(),
                            RpcErrorData::new(ApplicationErrorCode::Internal, false),
                        ),
                    ),
                    became_ready: false,
                },
            },
            Err(data) => SessionReply {
                response: RpcResponse::failure(
                    id,
                    RpcErrorObject::application("hello rejected", data),
                ),
                became_ready: false,
            },
        }
    }
}
