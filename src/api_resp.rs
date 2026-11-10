//! 与 Host `ApiResp` 对齐的轻量响应（插件不依赖 olink-core）。

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// 管理 API 统一包络
#[derive(Debug, Serialize)]
pub struct ApiResp<T> {
    pub code: i32,
    pub data: Option<T>,
    pub total: Option<u64>,
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
}

impl<T: Serialize> IntoResponse for ApiResp<T> {
    fn into_response(self) -> Response {
        let body = match serde_json::to_vec(&self) {
            Ok(v) => v,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
            }
        };
        let mut resp = body.into_response();
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        resp
    }
}

impl<T> ApiResp<T> {
    /// 成功带 data
    pub fn with_data(data: T) -> Self {
        Self {
            code: 200,
            data: Some(data),
            total: None,
            msg: "success".into(),
            error_type: None,
        }
    }

    /// 业务失败
    pub fn failure(msg: &str) -> Self {
        Self {
            code: -1,
            data: None,
            total: None,
            msg: msg.into(),
            error_type: Some("BUSINESS_ERROR".into()),
        }
    }

    /// 成功仅消息（任意 T）
    pub fn with_msg(msg: &str) -> Self {
        Self {
            code: 200,
            data: None,
            total: None,
            msg: msg.into(),
            error_type: None,
        }
    }
}
