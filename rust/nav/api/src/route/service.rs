//! service: 요청 검증, RouteDb 호출, 사고다발 구간 수집, 안내문과 경고 생성. HTTP(axum)는 모른다

use std::sync::Arc;
use std::time::Instant;

use routedb_core::route::{Profile, Route, RouteDb, Section};

use super::guidance;
use super::model::{Candidate, LatLon, RouteRequest, RouteResponse, RoutesResponse};
use crate::caution::construction;
use crate::caution::disaster;
use crate::caution::zone::{self, ZoneKind};
use crate::error::AppError;
use crate::state::RouteExtras;

/// 요청을 검증하고 블로킹 스레드에서 경로를 탐색한 뒤 경로가 지나는 사고다발 구간과 통행료를 더한다
pub async fn find_route(
    db: Arc<RouteDb>,
    extras: RouteExtras,
    input: RouteRequest,
) -> Result<RouteResponse, AppError> {
    validate_lat_lon(input.from)?;
    validate_lat_lon(input.to)?;
    // 탐색과 폴리곤 교차는 CPU 작업이므로 블로킹 스레드에서 실행한다
    let (route, elapsed_ms) = tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let cch_view = db.cch_view();
        let metrics = db.metrics();
        let mut router = db.router(&cch_view, &metrics);
        let route = router.route_with_heading(
            (input.from.lat, input.from.lon),
            input.from.heading,
            (input.to.lat, input.to.lon),
            input.profile,
        );
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        (
            route.map(|route| decorate(route, &extras, elapsed_ms)),
            elapsed_ms,
        )
    })
    .await
    .expect("탐색 작업이 중단됨");
    let _ = elapsed_ms;
    route.ok_or(AppError::RouteNotFound)
}

/// 경로에 사고다발 구간, 결빙·안개 구간, 보호구역, 통행료, 안내문, 경고를 더해 응답으로 만든다.
/// 어린이보호구역 구간은 경로의 school_zones에도 넣는다
fn decorate(mut route: Route, extras: &RouteExtras, elapsed_ms: f64) -> RouteResponse {
    let accident_sections = extras.accidents.sections(&route.geometry, route.distance_m);
    let hazard_sections = extras.hazards.sections(&route.geometry, route.distance_m);
    let icing_sections = extras.icing.sections(&route.geometry, route.distance_m);
    let fog_sections = extras.fog.sections(&route.geometry, route.distance_m);
    // 보호구역은 일반도로에 지정되므로 고속국도(101)·도시고속국도(102) 구간과 겹치는 부분은 뺀다
    let expressway: Vec<(f64, f64)> = route
        .segments
        .iter()
        .filter(|s| s.rank == 101 || s.rank == 102)
        .map(|s| (s.from_m, s.to_m))
        .collect();
    let protect_zones = zone::excluding(
        extras.zones.sections(&route.geometry, route.distance_m),
        &expressway,
    );
    route.school_zones = protect_zones
        .iter()
        .filter(|z| z.kind == ZoneKind::Child)
        .map(|z| Section {
            from_m: z.from_m,
            to_m: z.to_m,
        })
        .collect();
    // 어린이보호구역 구간은 30km/h보다 빠르게 지나지 않는 것으로 시간을 계산한다
    let school: Vec<(f64, f64)> = route
        .school_zones
        .iter()
        .map(|s| (s.from_m, s.to_m))
        .collect();
    route.duration_s += zone::school_delay_s(&route.speeds, &school);
    let enforce_sections = extras.enforce.sections(&route.pois);
    let rest_areas = extras
        .rest_areas
        .hits(&route.geometry, route.distance_m, &expressway);
    let risks = extras.risk.hits(&route.geometry, route.distance_m);
    let notices = extras
        .notices
        .hits(&route.links, &route.geometry, route.distance_m);
    let incidents = extras
        .incidents
        .hits(&route.links, &route.geometry, route.distance_m);
    let disasters = extras.disasters.hits(
        &route.links,
        &route.geometry,
        route.distance_m,
        &disaster::now_kst(),
    );
    let constructions =
        extras
            .constructions
            .hits(&route.geometry, route.distance_m, construction::today_kst());
    let toll = extras.tolls.fee(&route.tolls, 0);
    let instructions = guidance::instructions(&route, &extras.guide_lines);
    let cautions = guidance::Cautions {
        accident_sections: &accident_sections,
        hazard_sections: &hazard_sections,
        icing_sections: &icing_sections,
        fog_sections: &fog_sections,
        protect_zones: &protect_zones,
        enforce_sections: &enforce_sections,
        risks: &risks,
        constructions: &constructions,
        rest_areas: &rest_areas,
        notices: &notices,
        disasters: &disasters,
        incidents: &incidents,
    };
    let warnings = guidance::warnings(&route, &cautions, &toll);
    RouteResponse {
        route,
        accident_sections,
        protect_zones,
        toll,
        instructions,
        warnings,
        elapsed_ms,
    }
}

/// 최소시간 경로의 대안경로를 블로킹 스레드에서 찾아 응답으로 만든다
async fn find_alternatives(
    db: Arc<RouteDb>,
    extras: RouteExtras,
    base: Route,
    input: RouteRequest,
    count: usize,
) -> Vec<RouteResponse> {
    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let routes = db.alternatives(
            &base,
            (input.from.lat, input.from.lon),
            input.from.heading,
            (input.to.lat, input.to.lon),
            count,
        );
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        routes
            .into_iter()
            .map(|r| decorate(r, &extras, elapsed_ms))
            .collect()
    })
    .await
    .expect("대안 탐색 작업이 중단됨")
}

/// 대안경로 수
const ALTERNATIVE_COUNT: usize = 2;

/// 세 프로파일의 경로와 최소시간 경로의 대안 2개까지를 구해 같은 링크 열은 하나로 묶고, 가장 빠른·짧은·싼 후보를 표시한다.
/// 경로를 하나도 찾지 못하면 RouteNotFound이다. 요청의 profile은 쓰지 않는다
pub async fn find_routes(
    db: Arc<RouteDb>,
    extras: RouteExtras,
    input: RouteRequest,
) -> Result<RoutesResponse, AppError> {
    let mut candidates: Vec<Candidate> = Vec::new();
    for profile in [Profile::Time, Profile::Distance, Profile::Free] {
        let request = RouteRequest { profile, ..input };
        let found = find_route(Arc::clone(&db), extras.clone(), request).await;
        let route = match found {
            Ok(route) => route,
            Err(AppError::RouteNotFound) => continue,
            Err(e) => return Err(e),
        };
        match candidates
            .iter_mut()
            .find(|c| c.route.route.links == route.route.links)
        {
            Some(same) => same.profiles.push(profile),
            None => candidates.push(Candidate {
                profiles: vec![profile],
                alternative: 0,
                fastest: false,
                shortest: false,
                cheapest: false,
                route,
            }),
        }
    }
    if candidates.is_empty() {
        return Err(AppError::RouteNotFound);
    }
    // 최소시간 경로의 대안. 프로파일 경로와 같은 링크 열이면 넣지 않는다
    if let Some(base) = candidates
        .iter()
        .find(|c| c.profiles.contains(&Profile::Time))
        .map(|c| c.route.route.clone())
    {
        let alternatives = find_alternatives(
            Arc::clone(&db),
            extras.clone(),
            base,
            input,
            ALTERNATIVE_COUNT,
        )
        .await;
        let mut n = 0;
        for route in alternatives {
            if candidates
                .iter()
                .any(|c| c.route.route.links == route.route.links)
            {
                continue;
            }
            n += 1;
            candidates.push(Candidate {
                profiles: Vec::new(),
                alternative: n,
                fastest: false,
                shortest: false,
                cheapest: false,
                route,
            });
        }
    }
    let fastest = candidates
        .iter()
        .map(|c| c.route.route.duration_s)
        .fold(f64::INFINITY, f64::min);
    let shortest = candidates
        .iter()
        .map(|c| c.route.route.distance_m)
        .fold(f64::INFINITY, f64::min);
    let cheapest = candidates
        .iter()
        .filter(|c| c.route.toll.unknown.is_empty())
        .map(|c| c.route.toll.won)
        .min();
    for c in &mut candidates {
        c.fastest = c.route.route.duration_s == fastest;
        c.shortest = c.route.route.distance_m == shortest;
        c.cheapest = c.route.toll.unknown.is_empty() && Some(c.route.toll.won) == cheapest;
    }
    Ok(RoutesResponse { candidates })
}

/// 위도 -90~90, 경도 -180~180, 방위각 0~360 범위를 확인한다
fn validate_lat_lon(point: LatLon) -> Result<(), AppError> {
    if let Some(heading) = point.heading
        && !(0.0..=360.0).contains(&heading)
    {
        return Err(AppError::BadRequest(format!(
            "방위각 범위를 벗어남: {heading}"
        )));
    }
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
                lon: 127.0,
                heading: None
            }),
            Ok(())
        );
        assert!(
            validate_lat_lon(LatLon {
                lat: 37.5,
                lon: 127.0,
                heading: Some(361.0)
            })
            .is_err()
        );
        assert!(
            validate_lat_lon(LatLon {
                lat: 91.0,
                lon: 127.0,
                heading: None
            })
            .is_err()
        );
        assert!(
            validate_lat_lon(LatLon {
                lat: 37.5,
                lon: -181.0,
                heading: None
            })
            .is_err()
        );
    }
}
