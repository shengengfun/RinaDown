//! 稳定的本机服务应用错误契约。

use serde::{Deserialize, Serialize};

/// JSON-RPC 解析错误。
pub const PARSE_ERROR_CODE: i32 = -32700;
/// JSON-RPC 无效请求错误。
pub const INVALID_REQUEST_CODE: i32 = -32600;
/// JSON-RPC 方法不存在错误。
pub const METHOD_NOT_FOUND_CODE: i32 = -32601;
/// JSON-RPC 参数无效错误。
pub const INVALID_PARAMS_CODE: i32 = -32602;
/// JSON-RPC 内部错误。
pub const INTERNAL_ERROR_CODE: i32 = -32603;
/// FluxDown 应用错误使用的 JSON-RPC code。
pub const APPLICATION_ERROR_CODE: i32 = -32000;

/// 稳定应用错误码。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ApplicationErrorCode {
    ProtocolIncompatible,
    Unauthorized,
    InvalidArgument,
    NotFound,
    Conflict,
    Unavailable,
    Timeout,
    Cancelled,
    Unsupported,
    Internal,
}

/// FluxDown 应用错误的机器可读详情。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RpcErrorData {
    pub code: ApplicationErrorCode,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

impl RpcErrorData {
    /// 创建不带字段或版本上下文的错误详情。
    #[must_use]
    pub const fn new(code: ApplicationErrorCode, retryable: bool) -> Self {
        Self {
            code,
            retryable,
            field: None,
            revision: None,
        }
    }
}

/// JSON-RPC 2.0 错误对象。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RpcErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<RpcErrorData>,
}

impl RpcErrorObject {
    /// 创建带稳定 FluxDown 应用错误详情的错误对象。
    #[must_use]
    pub fn application(message: impl Into<String>, data: RpcErrorData) -> Self {
        Self {
            code: APPLICATION_ERROR_CODE,
            message: message.into(),
            data: Some(data),
        }
    }

    /// 创建不携带应用数据的 JSON 解析错误。
    #[must_use]
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self {
            code: PARSE_ERROR_CODE,
            message: message.into(),
            data: None,
        }
    }

    /// 创建不携带应用数据的无效请求错误。
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: INVALID_REQUEST_CODE,
            message: message.into(),
            data: None,
        }
    }
}
