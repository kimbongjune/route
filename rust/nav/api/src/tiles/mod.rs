//! 브이월드 2D 타일 프록시. 브라우저에 인증키를 두지 않기 위해 `GET /tiles/{layer}/{z}/{x}/{y}`를 받아 서버가 대신 요청한다

mod handler;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

/// 타일 라우트
pub fn router() -> Router<AppState> {
    Router::new().route("/tiles/{layer}/{z}/{x}/{y}", get(handler::tile))
}
