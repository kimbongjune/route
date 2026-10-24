use routedb_core::route::{Profile, Route};
use serde::{Deserialize, Serialize};

use super::guidance::{Instruction, Warning};
use super::toll::TollFee;
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
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct RouteRequest {
    pub from: LatLon,
    pub to: LatLon,
    #[serde(default = "default_profile")]
    pub profile: Profile,
}

fn default_profile() -> Profile {
    Profile::Distance
}

/// `POST /route` 응답. `Route`의 필드에 경로가 지나는 사고다발 구간, 통행료, 안내문, 경고, 탐색 시간을 더한다
#[derive(Serialize)]
pub struct RouteResponse {
    #[serde(flatten)]
    pub route: Route,
    pub accident_sections: Vec<AccidentSection>,
    /// 1종 기준 통행료. 구간별 요금과 요금을 정하지 못한 영업소
    pub toll: TollFee,
    /// 안내 지점마다의 안내문. `steps`와 같은 순서
    pub instructions: Vec<Instruction>,
    /// 경로 위 경고. 거리 순서이며 경로 전체 통과제한은 맨 앞, 요금소는 맨 뒤
    pub warnings: Vec<Warning>,
    pub elapsed_ms: f64,
}

/// `POST /routes`의 후보 하나. 같은 링크 열이 나온 프로파일은 하나로 묶는다
#[derive(Serialize)]
pub struct Candidate {
    /// 이 경로가 나온 프로파일. 둘 이상이면 같은 경로이고 대안경로는 비어 있다
    pub profiles: Vec<Profile>,
    /// 대안경로 번호. 프로파일 경로는 0이고 최소시간 경로의 대안은 1부터이다
    pub alternative: u8,
    /// 후보 가운데 소요시간이 가장 짧음
    pub fastest: bool,
    /// 후보 가운데 거리가 가장 짧음
    pub shortest: bool,
    /// 후보 가운데 통행료가 가장 적음(미확인 영업소가 없는 후보끼리)
    pub cheapest: bool,
    #[serde(flatten)]
    pub route: RouteResponse,
}

/// `POST /routes` 응답. 최소시간, 최단거리, 무료도로 세 프로파일의 경로와 최소시간 경로의 대안을 통행료와 함께 비교한다
#[derive(Serialize)]
pub struct RoutesResponse {
    pub candidates: Vec<Candidate>,
}
