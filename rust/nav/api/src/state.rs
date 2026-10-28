use std::sync::Arc;

use routedb_core::route::RouteDb;
use routedb_geocode::GeocodeIndex;

use crate::route::toll::TollTable;
use tokio::sync::broadcast;

use crate::accident::index::AccidentIndex;
use crate::caution::construction::ConstructionIndex;
use crate::caution::disaster::DisasterStore;
use crate::caution::enforce::EnforceTable;
use crate::caution::incident::IncidentStore;
use crate::caution::notice::NoticeStore;
use crate::caution::rest::RestAreaIndex;
use crate::caution::risk::RiskStore;
use crate::caution::section::SectionIndex;
use crate::caution::zone::ZoneIndex;
use crate::collector::vsl::VslStore;
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
    /// 도로환경 위험요소 폴리곤 색인(UTIC 교통안전정보)
    pub hazards: Arc<AccidentIndex>,
    /// 상습 결빙구간 색인
    pub icing: Arc<SectionIndex>,
    /// 안개 잦은 구간 색인
    pub fog: Arc<SectionIndex>,
    /// 어린이·노인·장애인 보호구역 색인
    pub zones: Arc<ZoneIndex>,
    /// 구간단속 시점·종점 카메라 짝
    pub enforce: Arc<EnforceTable>,
    /// 도로위험상황예보 지점. 폴링 태스크가 바꾼다
    pub risk: Arc<RiskStore>,
    /// 공사 지점(UTIC TCS 공사정보 중 위치를 찾은 것)
    pub constructions: Arc<ConstructionIndex>,
    /// 고속도로 휴게소
    pub rest_areas: Arc<RestAreaIndex>,
    /// 주의운전구간 실시간 메시지. 폴링 태스크가 바꾼다
    pub notices: Arc<NoticeStore>,
    /// 재난상황정보. 폴링 태스크가 바꾼다
    pub disasters: Arc<DisasterStore>,
    /// 돌발정보(국가교통정보센터·UTIC) 경고용 목록. 돌발정보 폴링 태스크가 바꾼다
    pub incidents: Arc<IncidentStore>,
    /// 제한속도가 낮아진 가변속도제한 링크. 가변속도제한 폴링 태스크가 바꾼다
    pub vsl: Arc<VslStore>,
    /// 지오코더 색인
    pub geocoder: Arc<GeocodeIndex>,
    /// 통행료 영업소·요금표
    pub tolls: Arc<TollTable>,
    /// 바깥 API 호출용 HTTP 클라이언트
    pub http: reqwest::Client,
    /// 브이월드 인증키. 배경지도 타일 프록시에 사용한다
    pub vworld_key: String,
    /// MapServer·MapCache 주소. 자체 벡터 타일 프록시에 사용한다
    pub mapserver_url: String,
    /// 배율 묶음을 적용 태스크로 보내는 채널
    pub updates: UpdateSender,
    /// 가중치 갱신 이벤트. `GET /events`가 구독한다
    pub events: broadcast::Sender<WeightEvent>,
}

/// 경로 응답에 경고와 통행료를 더할 때 사용하는 데이터
#[derive(Clone)]
pub struct RouteExtras {
    pub accidents: Arc<AccidentIndex>,
    pub guide_lines: Arc<GuideLineTable>,
    pub tolls: Arc<TollTable>,
    pub hazards: Arc<AccidentIndex>,
    pub icing: Arc<SectionIndex>,
    pub fog: Arc<SectionIndex>,
    pub zones: Arc<ZoneIndex>,
    pub enforce: Arc<EnforceTable>,
    pub risk: Arc<RiskStore>,
    pub constructions: Arc<ConstructionIndex>,
    pub rest_areas: Arc<RestAreaIndex>,
    pub notices: Arc<NoticeStore>,
    pub disasters: Arc<DisasterStore>,
    pub incidents: Arc<IncidentStore>,
    pub vsl: Arc<VslStore>,
}

impl AppState {
    pub fn route_extras(&self) -> RouteExtras {
        RouteExtras {
            accidents: Arc::clone(&self.accidents),
            guide_lines: Arc::clone(&self.guide_lines),
            tolls: Arc::clone(&self.tolls),
            hazards: Arc::clone(&self.hazards),
            icing: Arc::clone(&self.icing),
            fog: Arc::clone(&self.fog),
            zones: Arc::clone(&self.zones),
            enforce: Arc::clone(&self.enforce),
            risk: Arc::clone(&self.risk),
            constructions: Arc::clone(&self.constructions),
            rest_areas: Arc::clone(&self.rest_areas),
            notices: Arc::clone(&self.notices),
            disasters: Arc::clone(&self.disasters),
            incidents: Arc::clone(&self.incidents),
            vsl: Arc::clone(&self.vsl),
        }
    }
}
