use serde::{Deserialize, Serialize};

/// 검색 대상. 브이월드 검색 API의 type
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SearchType {
    /// 장소명. 건물·시설·기관명
    #[default]
    Place,
    /// 도로명주소
    Address,
}

/// `GET /geocode?q=검색어&type=place|address&size=10`
#[derive(Debug, Deserialize)]
pub struct GeocodeQuery {
    pub q: String,
    #[serde(default, rename = "type")]
    pub search_type: SearchType,
    #[serde(default = "default_size")]
    pub size: u32,
}

fn default_size() -> u32 {
    10
}

/// 검색 결과 한 건
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeocodeItem {
    /// 장소명. 주소 검색이면 도로명주소
    pub title: String,
    /// 도로명주소. 없으면 지번주소
    pub address: String,
    pub lat: f64,
    pub lon: f64,
}

/// `GET /geocode` 응답
#[derive(Debug, PartialEq, Serialize)]
pub struct GeocodeResponse {
    /// 브이월드가 찾은 전체 건수
    pub total: u32,
    pub items: Vec<GeocodeItem>,
}
