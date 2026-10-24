//! 타일 프록시. 브이월드 2D 타일은 브라우저에 인증키를 두지 않기 위해 `GET /tiles/{layer}/{z}/{x}/{y}`로,
//! 자체 벡터 타일은 GeoServer를 같은 출처로 보이기 위해 `GET /vtiles/{layer}/{z}/{x}/{y}`로 서버가 대신 요청한다

mod handler;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

/// 타일 라우트
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tiles/{layer}/{z}/{x}/{y}", get(handler::tile))
        .route("/vtiles/{layer}/{z}/{x}/{y}", get(handler::vector_tile))
}
