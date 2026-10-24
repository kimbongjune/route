//! 지오코딩 기능. 도로명주소 전자지도와 국가관심지점정보로 만든 색인 파일에서 주소·장소를 찾고 좌표를 주소로 바꾼다

mod handler;
pub mod model;
pub mod service;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

/// 지오코딩 기능의 라우트 모음
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/geocode", get(handler::search))
        .route("/reverse", get(handler::reverse))
}
