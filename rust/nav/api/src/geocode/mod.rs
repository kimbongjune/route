//! 지오코딩 기능. 브라우저에 인증키를 두지 않기 위해 서버가 브이월드 검색 API를 대신 호출한다

mod handler;
pub mod model;
pub mod service;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

/// 지오코딩 기능의 라우트 모음
pub fn router() -> Router<AppState> {
    Router::new().route("/geocode", get(handler::search))
}
