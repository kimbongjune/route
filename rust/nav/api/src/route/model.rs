use routedb_core::route::{Profile, Route};
use serde::{Deserialize, Serialize};

/// 위경도
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
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

/// `POST /route` 응답. `Route`의 필드에 탐색 시간을 더한다
#[derive(Serialize)]
pub struct RouteResponse {
    #[serde(flatten)]
    pub route: Route,
    pub elapsed_ms: f64,
}
