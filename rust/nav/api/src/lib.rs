//! 내비 API. 라우터 조립은 여기서 하며 main.rs와 통합 테스트가 같이 사용한다.

pub mod accident;
pub mod caution;
pub mod collector;
pub mod error;
pub mod extract;
pub mod geocode;
pub mod guideline;
pub mod response;
pub mod route;
pub mod state;
pub mod tiles;

use std::path::PathBuf;

use axum::Router;
use axum::extract::Request;
use axum::http::Uri;
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;

use crate::error::AppError;
use crate::state::AppState;

/// 프론트 정적 파일 폴더. 환경 변수 `NAV_WEB_DIR`이 있으면 그 폴더, 없으면 저장소의 nav/web이다
pub fn web_dir() -> PathBuf {
    std::env::var_os("NAV_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../web")))
}

/// 라우터 조립. API 경로가 아니면 nav/web의 정적 파일을 준다. 정적 파일은 고칠 때마다 바로 반영되도록 캐시하지 않게 한다.
/// 응답은 브라우저가 받으면 gzip으로 압축한다(벡터 타일·JSON. 이미지는 압축하지 않는다)
pub fn app(state: AppState) -> Router {
    let web = Router::new()
        .fallback_service(ServeDir::new(web_dir()).fallback(get(not_found)))
        .layer(middleware::from_fn(static_cache));
    Router::new()
        .merge(route::router())
        .merge(geocode::router())
        .merge(tiles::router())
        .route("/events", get(collector::handler::events))
        .fallback_service(web)
        .layer(CompressionLayer::new())
        .with_state(state)
}

/// 정적 파일 캐시. 글리프 PBF(/fonts/)는 바뀌지 않으므로 일주일 캐시하고, 나머지는 고칠 때마다 바로 반영되도록 캐시하지 않는다
async fn static_cache(request: Request, next: Next) -> Response {
    let fonts = request.uri().path().starts_with("/fonts/");
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if fonts {
            "public, max-age=604800"
        } else {
            "no-cache"
        }),
    );
    response
}

/// 등록되지 않은 주소. 404 NOT_FOUND. 글리프 PBF는 없는 범위이면 빈 PBF를 준다.
/// MapLibre는 라벨 글자의 범위 파일이 404이면 그 타일의 레이어를 그리지 않으므로 글자만 빠지게 한다
async fn not_found(uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/fonts/") && path.ends_with(".pbf") {
        return (
            [(header::CONTENT_TYPE, "application/x-protobuf")],
            Vec::<u8>::new(),
        )
            .into_response();
    }
    AppError::NotFound(path.to_string()).into_response()
}
