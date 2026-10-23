//! 내비 API. 라우터 조립은 여기서 하며 main.rs와 통합 테스트가 같이 사용한다.

pub mod error;
pub mod extract;
pub mod geocode;
pub mod response;
pub mod route;
pub mod state;

use axum::Router;
use axum::http::Uri;

use crate::error::AppError;
use crate::state::AppState;

/// 라우터 조립
pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(route::router())
        .merge(geocode::router())
        .fallback(not_found)
        .with_state(state)
}

/// 등록되지 않은 주소. 404 NOT_FOUND
async fn not_found(uri: Uri) -> AppError {
    AppError::NotFound(uri.path().to_string())
}
