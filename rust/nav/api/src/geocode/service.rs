//! service: 검색어 검증, 색인 검색, 응답 변환. HTTP 서버 쪽(axum)은 모른다

use routedb_geocode::{GeocodeIndex, Hit, Kind};

use super::model::{GeocodeItem, GeocodeQuery, GeocodeResponse};
use crate::error::AppError;

const MAX_SIZE: u32 = 100;
/// 역지오코딩에서 주소를 찾는 반경(m)
pub const REVERSE_RADIUS_M: f64 = 300.0;

fn item(hit: Hit) -> GeocodeItem {
    GeocodeItem {
        kind: match hit.kind {
            Kind::Address => "address",
            Kind::Place => "place",
        },
        title: hit.title,
        address: hit.address,
        lat: hit.lat,
        lon: hit.lon,
    }
}

fn validate_point(lat: f64, lon: f64) -> Result<(), AppError> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(AppError::BadRequest(format!(
            "좌표 범위를 벗어남: {lat}, {lon}"
        )));
    }
    Ok(())
}

/// 검색어를 검증하고 색인에서 주소와 장소를 찾는다
pub fn search(index: &GeocodeIndex, query: GeocodeQuery) -> Result<GeocodeResponse, AppError> {
    let q = query.q.trim();
    if q.is_empty() {
        return Err(AppError::BadRequest("q가 비어 있음".to_string()));
    }
    let center = match (query.lat, query.lon) {
        (Some(lat), Some(lon)) => {
            validate_point(lat, lon)?;
            Some((lat, lon))
        }
        (None, None) => None,
        _ => {
            return Err(AppError::BadRequest(
                "lat과 lon은 같이 있어야 함".to_string(),
            ));
        }
    };
    let size = query.size.clamp(1, MAX_SIZE) as usize;
    let found = index.search(q, center, size);
    Ok(GeocodeResponse {
        total: found.total,
        items: found.hits.into_iter().map(item).collect(),
    })
}

/// 좌표에서 가장 가까운 주소. 반경 안에 없으면 404
pub fn reverse(index: &GeocodeIndex, lat: f64, lon: f64) -> Result<GeocodeItem, AppError> {
    validate_point(lat, lon)?;
    index
        .nearest_address(lat, lon, REVERSE_RADIUS_M)
        .map(item)
        .ok_or(AppError::AddressNotFound)
}
