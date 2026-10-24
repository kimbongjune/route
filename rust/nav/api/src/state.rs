use std::sync::Arc;

use routedb_core::route::RouteDb;
use routedb_geocode::GeocodeIndex;

use crate::route::toll::TollTable;
use tokio::sync::broadcast;

use crate::accident::index::AccidentIndex;
use crate::collector::{UpdateSender, WeightEvent};
use crate::guideline::GuideLineTable;

/// 앱 전체에서 공유하는 상태. `RouteDb`와 `AccidentIndex`는 `Arc`, `reqwest::Client`와 채널은 내부가 `Arc`라 Clone이 가볍다
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<RouteDb>,
    /// 사고다발 폴리곤 색인
    pub accidents: Arc<AccidentIndex>,
    /// 노면색깔유도선 표
    pub guide_lines: Arc<GuideLineTable>,
    /// 지오코더 색인
    pub geocoder: Arc<GeocodeIndex>,
    /// 통행료 영업소·요금표
    pub tolls: Arc<TollTable>,
    /// 바깥 API 호출용 HTTP 클라이언트
    pub http: reqwest::Client,
    /// 브이월드 인증키. 배경지도 타일 프록시에 사용한다
    pub vworld_key: String,
    /// GeoServer 주소. 자체 벡터 타일 프록시에 사용한다
    pub geoserver_url: String,
    /// 배율 묶음을 적용 태스크로 보내는 채널
    pub updates: UpdateSender,
    /// 가중치 갱신 이벤트. `GET /events`가 구독한다
    pub events: broadcast::Sender<WeightEvent>,
}
