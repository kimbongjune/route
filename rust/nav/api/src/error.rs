use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::response::ApiResponse;

/// 앱 전체 공통 에러. 핸들러가 반환하면 `ApiResponse`의 실패 응답으로 바뀐다
#[derive(Debug, PartialEq)]
pub enum AppError {
    /// 요청 값이 잘못됨. 400 BAD_REQUEST
    BadRequest(String),
    /// 경로 없음. 404 ROUTE_NOT_FOUND
    RouteNotFound,
    /// 없는 주소. 404 NOT_FOUND
    NotFound(String),
    /// 브이월드 같은 바깥 서비스 호출 실패. 502 UPSTREAM_ERROR
    Upstream(String),
}

impl AppError {
    /// 상태 코드, 결과 코드, 메시지
    pub fn parts(self) -> (StatusCode, &'static str, String) {
        match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", message),
            AppError::RouteNotFound => (
                StatusCode::NOT_FOUND,
                "ROUTE_NOT_FOUND",
                "경로 없음".to_string(),
            ),
            AppError::NotFound(path) => (
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                format!("없는 주소: {path}"),
            ),
            AppError::Upstream(message) => (StatusCode::BAD_GATEWAY, "UPSTREAM_ERROR", message),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        ApiResponse::fail(status, code, message).into_response()
    }
}
