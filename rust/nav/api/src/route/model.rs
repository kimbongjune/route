use routedb_core::route::{Profile, Route};
use serde::{Deserialize, Serialize};

use super::guidance::{Instruction, Warning};
use crate::accident::model::AccidentSection;

/// 위경도. `heading`은 진행 방위각(진북 0, 시계 방향 0~360도)이며 출발지에서만 사용한다
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub heading: Option<f64>,
}

/// `POST /route` 요청 본문. 프로파일은 distance, time, free이며 없으면 distance이다
#[derive(Debug, Deserialize)]
pub struct RouteRequest {
    pub from: LatLon,
    pub to: LatLon,
    #[serde(default = "default_profile")]
    pub profile: Profile,
}

fn default_profile() -> Profile {
    Profile::Distance
}

/// `POST /route` 응답. `Route`의 필드에 경로가 지나는 사고다발 구간, 안내문, 경고, 탐색 시간을 더한다
#[derive(Serialize)]
pub struct RouteResponse {
    #[serde(flatten)]
    pub route: Route,
    pub accident_sections: Vec<AccidentSection>,
    /// 안내 지점마다의 안내문. `steps`와 같은 순서
    pub instructions: Vec<Instruction>,
    /// 경로 위 경고. 거리 순서이며 경로 전체 통과제한은 맨 앞, 요금소는 맨 뒤
    pub warnings: Vec<Warning>,
    pub elapsed_ms: f64,
}
