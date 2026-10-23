use std::sync::Arc;

use routedb_core::route::RouteDb;

/// 앱 전체에서 공유하는 상태. `RouteDb`는 `Arc`, `reqwest::Client`는 내부가 `Arc`라 Clone이 가볍다
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<RouteDb>,
    /// 브이월드 호출용 HTTP 클라이언트
    pub http: reqwest::Client,
    /// 브이월드 인증키
    pub vworld_key: String,
}
