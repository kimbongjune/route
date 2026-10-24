//! 경로 기능. handler가 HTTP를 받고 service가 RouteDb를 호출한다

pub mod guidance;
mod handler;
pub mod model;
pub mod service;
pub mod toll;

use axum::Router;
use axum::routing::post;

use crate::state::AppState;

/// 경로 기능의 라우트 모음
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/route", post(handler::create))
        .route("/routes", post(handler::compare))
}
