//! JSON-RPC 2.0 信封与本机服务握手。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ApplicationErrorCode, RpcErrorData, RpcErrorObject};

/// JSON-RPC wire 版本。
pub const JSONRPC_VERSION: &str = "2.0";
/// 当前本机服务协议版本。
pub const PROTOCOL_VERSION: u32 = 2;
/// 本机服务接受的最低协议版本。
pub const MIN_PROTOCOL_VERSION: u32 = 2;

/// 本机服务在 FluxDown 架构中的职责。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ServiceRole {
    Daemon,
    Agent,
}

/// JSON-RPC 请求 ID。只接受字符串或有符号 64 位整数。
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Integer(i64),
}

/// 客户端首帧 `system.hello` 的版本与能力协商信息。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ClientHello {
    pub client_name: String,
    pub client_version: String,
    pub min_protocol_version: u32,
    pub max_protocol_version: u32,
    pub requested_role: ServiceRole,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// 服务端成功协商后返回的版本与能力信息。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServiceHello {
    pub role: ServiceRole,
    pub service_name: String,
    pub service_version: String,
    pub protocol_version: u32,
    pub instance_id: String,
    pub capabilities: Vec<String>,
}

impl ServiceHello {
    /// 创建协议 v2 握手响应。
    #[must_use]
    pub fn new(
        role: ServiceRole,
        service_name: impl Into<String>,
        service_version: impl Into<String>,
        instance_id: impl Into<String>,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            role,
            service_name: service_name.into(),
            service_version: service_version.into(),
            protocol_version: PROTOCOL_VERSION,
            instance_id: instance_id.into(),
            capabilities,
        }
    }
}

/// 协商客户端与服务端共同支持的最高协议版本。
pub fn negotiate_protocol(hello: &ClientHello) -> Result<u32, RpcErrorData> {
    let minimum = hello.min_protocol_version.max(MIN_PROTOCOL_VERSION);
    let maximum = hello.max_protocol_version.min(PROTOCOL_VERSION);
    if minimum <= maximum {
        Ok(maximum)
    } else {
        Err(RpcErrorData::new(
            ApplicationErrorCode::ProtocolIncompatible,
            false,
        ))
    }
}

/// 带 ID 的 JSON-RPC 调用请求。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl RpcRequest {
    /// 创建符合 JSON-RPC 2.0 的调用请求。
    #[must_use]
    pub fn new(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            method: method.into(),
            params,
        }
    }

    /// 校验固定 JSON-RPC 版本。
    pub fn validate(&self) -> Result<(), RpcErrorData> {
        validate_jsonrpc_version(&self.jsonrpc)
    }
}

/// 校验连接首个 RPC 调用并解析 `system.hello`。
///
/// 服务端必须在读取任何其他方法前调用此函数；成功后才可进入正常分发。
pub fn validate_first_request(
    request: &RpcRequest,
    expected_role: ServiceRole,
) -> Result<ClientHello, RpcErrorData> {
    request.validate()?;
    if request.method != crate::method::SYSTEM_HELLO {
        return Err(invalid_rpc_field("method"));
    }
    let Some(params) = request.params.clone() else {
        return Err(invalid_rpc_field("params"));
    };
    let hello =
        serde_json::from_value::<ClientHello>(params).map_err(|_| invalid_rpc_field("params"))?;
    if hello.requested_role != expected_role {
        return Err(RpcErrorData::new(
            ApplicationErrorCode::ProtocolIncompatible,
            false,
        ));
    }
    negotiate_protocol(&hello)?;
    Ok(hello)
}

/// 不带 ID 的 JSON-RPC 通知。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl RpcNotification {
    /// 创建符合 JSON-RPC 2.0 的通知。
    #[must_use]
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            method: method.into(),
            params,
        }
    }

    /// 校验固定 JSON-RPC 版本。
    pub fn validate(&self) -> Result<(), RpcErrorData> {
        validate_jsonrpc_version(&self.jsonrpc)
    }
}

/// 一个已分型的 JSON-RPC 入站帧。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum RpcIncoming {
    Request(RpcRequest),
    Notification(RpcNotification),
}

/// 成功响应；wire 上永远没有 `error` 字段。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RpcSuccessResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    pub result: Value,
}

/// 失败响应；wire 上永远没有 `result` 字段。`id: null` 只用于无法恢复 ID 的解析或无效请求。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RpcFailureResponse {
    pub jsonrpc: String,
    pub id: Option<RequestId>,
    pub error: RpcErrorObject,
}

/// 互斥的 JSON-RPC 成功或失败响应。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum RpcResponse {
    Success(RpcSuccessResponse),
    Failure(RpcFailureResponse),
}

impl RpcResponse {
    /// 创建成功响应。
    #[must_use]
    pub fn success(id: RequestId, result: Value) -> Self {
        Self::Success(RpcSuccessResponse {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result,
        })
    }

    /// 创建带可恢复请求 ID 的失败响应。
    #[must_use]
    pub fn failure(id: RequestId, error: RpcErrorObject) -> Self {
        Self::Failure(RpcFailureResponse {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some(id),
            error,
        })
    }

    /// 创建无法恢复请求 ID 的 JSON 解析错误响应。
    #[must_use]
    pub fn parse_failure(message: impl Into<String>) -> Self {
        Self::Failure(RpcFailureResponse {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: None,
            error: RpcErrorObject::parse_error(message),
        })
    }

    /// 创建无法恢复请求 ID 的无效请求响应。
    #[must_use]
    pub fn invalid_request_failure(message: impl Into<String>) -> Self {
        Self::Failure(RpcFailureResponse {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: None,
            error: RpcErrorObject::invalid_request(message),
        })
    }
}

fn validate_jsonrpc_version(version: &str) -> Result<(), RpcErrorData> {
    if version == JSONRPC_VERSION {
        Ok(())
    } else {
        Err(invalid_rpc_field("jsonrpc"))
    }
}

fn invalid_rpc_field(field: &str) -> RpcErrorData {
    RpcErrorData {
        code: ApplicationErrorCode::InvalidArgument,
        retryable: false,
        field: Some(field.to_owned()),
        revision: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        ClientHello, MIN_PROTOCOL_VERSION, PROTOCOL_VERSION, RequestId, RpcIncoming, RpcRequest,
        RpcResponse, ServiceHello, ServiceRole, negotiate_protocol, validate_first_request,
    };
    use crate::error::{ApplicationErrorCode, RpcErrorData, RpcErrorObject};

    #[test]
    fn negotiates_protocol_v2_and_stable_hello_shape() -> Result<(), serde_json::Error> {
        let client = ClientHello {
            client_name: "fluxdown-desktop".to_owned(),
            client_version: "1.0.0".to_owned(),
            min_protocol_version: MIN_PROTOCOL_VERSION,
            max_protocol_version: PROTOCOL_VERSION,
            requested_role: ServiceRole::Agent,
            capabilities: vec!["client.selections".to_owned()],
        };
        assert_eq!(negotiate_protocol(&client), Ok(PROTOCOL_VERSION));

        let service = ServiceHello::new(
            ServiceRole::Agent,
            "fluxdown-agent",
            "1.0.0",
            "instance-1",
            vec!["agent.gateway".to_owned()],
        );
        let wire = serde_json::to_value(service)?;
        assert_eq!(
            wire,
            json!({
                "role": "agent",
                "serviceName": "fluxdown-agent",
                "serviceVersion": "1.0.0",
                "protocolVersion": 2,
                "instanceId": "instance-1",
                "capabilities": ["agent.gateway"]
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_non_intersecting_protocol_range() {
        let client = ClientHello {
            client_name: "old-client".to_owned(),
            client_version: "0.1.0".to_owned(),
            min_protocol_version: 1,
            max_protocol_version: 1,
            requested_role: ServiceRole::Daemon,
            capabilities: Vec::new(),
        };

        let error = match negotiate_protocol(&client) {
            Ok(version) => panic!("v1 unexpectedly negotiated protocol {version}"),
            Err(error) => error,
        };
        assert_eq!(error.code, ApplicationErrorCode::ProtocolIncompatible);
        assert!(!error.retryable);
    }

    #[test]
    fn deserializes_requested_role_and_capabilities() -> Result<(), serde_json::Error> {
        let wire = json!({
            "clientName": "web",
            "clientVersion": "1",
            "minProtocolVersion": 2,
            "maxProtocolVersion": 2,
            "requestedRole": "daemon",
            "capabilities": []
        });
        let hello = serde_json::from_value::<ClientHello>(wire)?;
        assert_eq!(hello.requested_role, ServiceRole::Daemon);
        assert_eq!(hello.capabilities, Vec::<String>::new());
        assert_eq!(
            serde_json::to_value(hello)?["maxProtocolVersion"],
            Value::from(2)
        );
        Ok(())
    }

    #[test]
    fn requires_system_hello_as_first_request() {
        let request = RpcRequest::new(RequestId::Integer(-7), "system.ping", None);
        let error = match validate_first_request(&request, ServiceRole::Agent) {
            Ok(_) => panic!("non-hello first request unexpectedly passed"),
            Err(error) => error,
        };
        assert_eq!(error.code, ApplicationErrorCode::InvalidArgument);
        assert_eq!(error.field.as_deref(), Some("method"));
    }

    #[test]
    fn validates_hello_role_and_protocol() {
        let request = RpcRequest::new(
            RequestId::String("hello".to_owned()),
            crate::method::SYSTEM_HELLO,
            Some(json!({
                "clientName": "desktop",
                "clientVersion": "1",
                "minProtocolVersion": 2,
                "maxProtocolVersion": 2,
                "requestedRole": "agent",
                "capabilities": ["client.selections"]
            })),
        );
        let hello = match validate_first_request(&request, ServiceRole::Agent) {
            Ok(hello) => hello,
            Err(error) => panic!("valid hello rejected: {:?}", error.code),
        };
        assert_eq!(hello.client_name, "desktop");
    }

    #[test]
    fn rejects_fractional_out_of_range_and_notification_ids() {
        assert!(
            serde_json::from_value::<RpcIncoming>(json!({
                "jsonrpc": "2.0", "id": 1.5, "method": "system.ping"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RpcIncoming>(json!({
                "jsonrpc": "2.0", "id": 9223372036854775808_u64, "method": "system.ping"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RpcIncoming>(json!({
                "jsonrpc": "2.0", "id": 1, "method": "service.event", "params": {}
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<super::RpcNotification>(json!({
                "jsonrpc": "2.0", "id": 1, "method": "service.event", "params": {}
            }))
            .is_err()
        );
    }

    #[test]
    fn response_variants_are_mutually_exclusive_and_null_id_is_scoped()
    -> Result<(), serde_json::Error> {
        let success = serde_json::to_value(RpcResponse::success(
            RequestId::Integer(-9),
            json!({"ok": true}),
        ))?;
        assert_eq!(success["id"], -9);
        assert!(success.get("error").is_none());

        let failure = serde_json::to_value(RpcResponse::failure(
            RequestId::String("x".to_owned()),
            RpcErrorObject::application(
                "conflict",
                RpcErrorData::new(ApplicationErrorCode::Conflict, false),
            ),
        ))?;
        assert!(failure.get("result").is_none());
        assert_eq!(failure["error"]["data"]["code"], "conflict");

        let parse = serde_json::to_value(RpcResponse::parse_failure("invalid json"))?;
        assert!(parse["id"].is_null());
        assert_eq!(parse["error"]["code"], -32700);
        Ok(())
    }
}
