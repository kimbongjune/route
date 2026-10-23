//! service: 요청 검증, RouteDb 호출, 사고다발 구간 수집, 안내문과 경고 생성. HTTP(axum)는 모른다

use std::sync::Arc;
use std::time::Instant;

use routedb_core::route::RouteDb;

use super::guidance;
use super::model::{LatLon, RouteRequest, RouteResponse};
use crate::accident::index::AccidentIndex;
use crate::error::AppError;

/// 요청을 검증하고 블로킹 스레드에서 경로를 탐색한 뒤 경로가 지나는 사고다발 구간을 붙인다
pub async fn find_route(
    db: Arc<RouteDb>,
    accidents: Arc<AccidentIndex>,
    input: RouteRequest,
) -> Result<RouteResponse, AppError> {
    validate_lat_lon(input.from)?;
    validate_lat_lon(input.to)?;
    // 탐색과 폴리곤 교차는 CPU 작업이므로 블로킹 스레드에서 실행한다
    let (route, elapsed_ms) = tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let cch_view = db.cch_view();
        let mut router = db.router(&cch_view);
        let route = router.route(
            (input.from.lat, input.from.lon),
            (input.to.lat, input.to.lon),
            input.profile,
        );
        let route = route.map(|route| {
            let accident_sections = accidents.sections(&route.geometry, route.distance_m);
            let instructions = guidance::instructions(&route);
            let warnings = guidance::warnings(&route, &accident_sections);
            (route, accident_sections, instructions, warnings)
        });
        (route, started.elapsed().as_secs_f64() * 1000.0)
    })
    .await
    .expect("탐색 작업이 중단됨");
    let (route, accident_sections, instructions, warnings) =
        route.ok_or(AppError::RouteNotFound)?;
    Ok(RouteResponse {
        route,
        accident_sections,
        instructions,
        warnings,
        elapsed_ms,
    })
}

/// 위도 -90~90, 경도 -180~180 범위를 확인한다
fn validate_lat_lon(point: LatLon) -> Result<(), AppError> {
    if !(-90.0..=90.0).contains(&point.lat) || !(-180.0..=180.0).contains(&point.lon) {
        return Err(AppError::BadRequest(format!(
            "위경도 범위를 벗어남: {}, {}",
            point.lat, point.lon
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lat_lon_out_of_range_is_rejected() {
        assert_eq!(
            validate_lat_lon(LatLon {
                lat: 37.5,
                lon: 127.0
            }),
            Ok(())
        );
        assert!(
            validate_lat_lon(LatLon {
                lat: 91.0,
                lon: 127.0
            })
            .is_err()
        );
        assert!(
            validate_lat_lon(LatLon {
                lat: 37.5,
                lon: -181.0
            })
            .is_err()
        );
    }
}
