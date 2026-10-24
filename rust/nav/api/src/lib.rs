//! 내비 API. 라우터 조립은 여기서 하며 main.rs와 통합 테스트가 같이 사용한다.

pub mod accident;
pub mod collector;
pub mod error;
pub mod extract;
pub mod geocode;
pub mod guideline;
pub mod response;
pub mod route;
pub mod state;
pub mod tiles;

use axum::Router;
use axum::http::Uri;
use axum::http::{HeaderValue, header};
use axum::routing::get;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::error::AppError;
use crate::state::AppState;

/// 프론트 정적 파일 폴더. nav/web
pub const WEB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web");

/// 라우터 조립. API 경로가 아니면 nav/web의 정적 파일을 준다. 정적 파일은 고칠 때마다 바로 반영되도록 캐시하지 않게 한다
pub fn app(state: AppState) -> Router {
    let web = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .service(ServeDir::new(WEB_DIR).fallback(get(not_found)));
    Router::new()
        .merge(route::router())
        .merge(geocode::router())
        .merge(tiles::router())
        .route("/events", get(collector::handler::events))
        .fallback_service(web)
        .with_state(state)
}

/// 등록되지 않은 주소. 404 NOT_FOUND
async fn not_found(uri: Uri) -> AppError {
    AppError::NotFound(uri.path().to_string())
}
