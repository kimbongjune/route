use serde::{Deserialize, Serialize};

/// `GET /geocode?q=검색어&size=10&lat=37.5&lon=127.0`. lat·lon은 지도 중심이며 같은 점수의 장소를 가까운 순으로 정렬한다
#[derive(Debug, Default, Deserialize)]
pub struct GeocodeQuery {
    pub q: String,
    #[serde(default = "default_size")]
    pub size: u32,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

fn default_size() -> u32 {
    10
}

/// `GET /reverse?lat=37.5&lon=127.0`
#[derive(Debug, Deserialize)]
pub struct ReverseQuery {
    pub lat: f64,
    pub lon: f64,
}

/// 검색 결과 한 건
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeocodeItem {
    /// address 주소, place 장소
    pub kind: &'static str,
    /// 장소명. 주소 검색이면 도로명주소
    pub title: String,
    /// 장소의 도로명주소. 주소면 지번주소와 건물명
    pub address: String,
    pub lat: f64,
    pub lon: f64,
}

/// `GET /geocode` 응답
#[derive(Debug, PartialEq, Serialize)]
pub struct GeocodeResponse {
    /// 검색어에 맞는 전체 건수
    pub total: usize,
    pub items: Vec<GeocodeItem>,
}
