//! 모든 응답의 공통 구조. 성공과 실패가 같은 모양이며 HTTP 상태 코드를 본문의 `status`에도 넣는다

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// 응답 본문. `data`는 성공일 때만 있고 실패일 때는 `code`와 `message`가 원인을 설명한다
#[derive(Debug, PartialEq, Serialize)]
pub struct ApiResponse<T> {
    /// HTTP 상태 코드
    pub status: u16,
    /// 결과 코드. 성공은 OK, 실패는 BAD_REQUEST, ROUTE_NOT_FOUND, NOT_FOUND, UPSTREAM_ERROR
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    /// 200 OK 성공 응답
    pub fn ok(data: T) -> Self {
        Self {
            status: StatusCode::OK.as_u16(),
            code: "OK",
            message: "성공".to_string(),
            data: Some(data),
        }
    }
}

impl ApiResponse<()> {
    /// 실패 응답. `data`는 없다
    pub fn fail(status: StatusCode, code: &'static str, message: String) -> Self {
        Self {
            status: status.as_u16(),
            code,
            message,
            data: None,
        }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).expect("상태 코드가 잘못됨");
        (status, Json(self)).into_response()
    }
}
