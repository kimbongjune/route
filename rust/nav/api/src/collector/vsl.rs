//! 가변속도제한(국가교통정보센터 `/vslInfo`). 구간마다 현재 제한속도와 기본 제한속도를 주기적으로 받아, 제한속도가 낮아진 링크는
//! 시간 배율(표준노드링크 제한속도 ÷ 가변 제한속도)을 가중치에 반영하고 경로가 그 링크를 지나면 경고한다

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use routedb_core::route::{FactorKind, RouteDb, WeightUpdate};
use serde::Deserialize;
use serde_json::Value;

use super::{Batch, Freshness, UpdateSender};
use crate::caution::{nearest_on_route, result_code, route_position};

const VSL_URL: &str = "https://openapi.its.go.kr:9443/vslInfo";
/// 경로가 링크를 지나도 링크 위치가 경로에서 이 거리보다 멀면 경고하지 않는다(m)
pub const VSL_LINK_M: f64 = 300.0;
/// 같은 제한속도의 경고가 이 거리 안에 이어지면 하나로 합친다(m)
pub const VSL_GAP_M: f64 = 1_000.0;
/// 시간 배율 상한
const MAX_VSL_FACTOR: f32 = 10.0;

/// 응답 항목 하나. 값은 모두 문자열이다
#[derive(Debug, Default, Deserialize)]
struct Item {
    #[serde(rename = "linkId", default)]
    link: String,
    #[serde(rename = "limitSpeed", default)]
    limit: String,
    #[serde(rename = "defLmtSpeed", default)]
    default: String,
    #[serde(rename = "coordX", default)]
    x: String,
    #[serde(rename = "coordY", default)]
    y: String,
}

/// 링크 하나의 가변속도제한. 한 링크에 구간이 여러 개이면 가장 낮은 제한속도이다
#[derive(Clone, Debug, PartialEq)]
pub struct VslLink {
    pub link: u64,
    /// 현재 제한속도(km/h)
    pub limit: u16,
    /// 기본 제한속도(km/h)
    pub default: u16,
    pub lat: f64,
    pub lon: f64,
    /// 표준노드링크 링크 좌표 (위도, 경도). 폴링 태스크가 그래프에서 채운다. 응답 좌표가 링크에서 수백 m 떨어진 행이 있어 경고 위치에 사용한다
    pub points: Vec<(f64, f64)>,
}

impl VslLink {
    /// 제한속도가 기본보다 낮아졌는지
    pub fn lowered(&self) -> bool {
        self.limit < self.default
    }
}

/// 경로가 지나는 가변속도 링크
#[derive(Clone, Debug, PartialEq)]
pub struct VslHit {
    pub at_m: f64,
    pub vsl: VslLink,
}

/// 위도·경도. 응답의 coordX·coordY는 행마다 위도와 경도가 뒤섞여 있어(2026-10-28 확인) 값의 범위로 구분한다
fn lat_lon(x: f64, y: f64) -> Option<(f64, f64)> {
    let is_lat = |v: f64| (32.0..=39.5).contains(&v);
    let is_lon = |v: f64| (124.0..=132.0).contains(&v);
    if is_lat(y) && is_lon(x) {
        Some((y, x))
    } else if is_lat(x) && is_lon(y) {
        Some((x, y))
    } else {
        None
    }
}

/// 응답 본문을 링크별 가변속도제한으로 바꾼다. 링크ID나 속도가 숫자가 아닌 행은 뺀다
pub fn parse(body: &Value) -> Result<Vec<VslLink>, String> {
    result_code(body, "가변속도제한")?;
    let items = body["body"]["items"].clone();
    if items.is_null() || items.as_str() == Some("") {
        return Ok(Vec::new());
    }
    let items: Vec<Item> =
        serde_json::from_value(items).map_err(|e| format!("가변속도제한 항목 해석 실패: {e}"))?;
    let mut by_link: HashMap<u64, VslLink> = HashMap::new();
    for i in items {
        let (Ok(link), Ok(limit), Ok(default)) = (
            i.link.trim().parse::<u64>(),
            i.limit.trim().parse::<u16>(),
            i.default.trim().parse::<u16>(),
        ) else {
            continue;
        };
        if limit == 0 {
            continue;
        }
        let (lat, lon) = lat_lon(
            i.x.trim().parse().unwrap_or(0.0),
            i.y.trim().parse().unwrap_or(0.0),
        )
        .unwrap_or((0.0, 0.0));
        let next = VslLink {
            link,
            limit,
            default,
            lat,
            lon,
            points: Vec::new(),
        };
        match by_link.get(&link) {
            Some(old) if old.limit <= limit => {}
            _ => {
                by_link.insert(link, next);
            }
        }
    }
    let mut out: Vec<VslLink> = by_link.into_values().collect();
    out.sort_by_key(|v| v.link);
    Ok(out)
}

/// 링크별 가변속도 배율. 낮아진 링크는 표준노드링크 제한속도 ÷ 가변 제한속도(1.0~MAX_VSL_FACTOR)이고, 낮아지지 않은 링크와
/// 이전에 낮아졌다가 목록에서 빠진 링크(previous)는 1.0이다. 그래프에 없는 링크는 뺀다
pub fn to_updates(links: &[VslLink], previous: &HashSet<u64>, db: &RouteDb) -> Vec<WeightUpdate> {
    let max_spd = db.file.max_spd();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in links {
        let Some(e) = db.edge_of_link(v.link) else {
            continue;
        };
        let factor = if v.lowered() {
            (max_spd[e as usize] as f32 / v.limit as f32).clamp(1.0, MAX_VSL_FACTOR)
        } else {
            1.0
        };
        seen.insert(v.link);
        out.push(WeightUpdate {
            link_id: v.link,
            vsl_factor: Some(factor),
            ..Default::default()
        });
    }
    for &link in previous.difference(&seen) {
        out.push(WeightUpdate {
            link_id: link,
            vsl_factor: Some(1.0),
            ..Default::default()
        });
    }
    out
}

/// 제한속도가 낮아진 링크 목록. 폴링 태스크가 바꾼다
#[derive(Default)]
pub struct VslStore {
    lowered: RwLock<Vec<VslLink>>,
}

impl VslStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace(&self, lowered: Vec<VslLink>) {
        *self.lowered.write().expect("가변속도 잠금 실패") = lowered;
    }

    /// 경로가 지나는 낮아진 링크. 링크 좌표 중 경로에서 VSL_LINK_M 안인 첫 점의 위치이며(링크 좌표가 없으면 응답 좌표),
    /// 거리 순서이고 같은 제한속도가 VSL_GAP_M 안에 이어지면 앞의 것만 남긴다
    pub fn hits(&self, links: &[u64], geometry: &[(f64, f64)], distance_m: f64) -> Vec<VslHit> {
        let guard = self.lowered.read().expect("가변속도 잠금 실패");
        if guard.is_empty() {
            return Vec::new();
        }
        let on_route: HashSet<u64> = links.iter().copied().collect();
        let position = route_position(geometry, distance_m);
        let mut hits: Vec<VslHit> = guard
            .iter()
            .filter(|v| on_route.contains(&v.link))
            .filter_map(|v| {
                let points = if v.points.is_empty() {
                    vec![(v.lat, v.lon)]
                } else {
                    v.points.clone()
                };
                let at_m = points.iter().find_map(|&(lat, lon)| {
                    let (d, at_m) = nearest_on_route(geometry, &position, lat, lon)?;
                    (lat != 0.0 && d <= VSL_LINK_M).then_some(at_m)
                })?;
                Some(VslHit {
                    at_m,
                    vsl: v.clone(),
                })
            })
            .collect();
        hits.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
        let mut out: Vec<VslHit> = Vec::new();
        for h in hits {
            if out
                .last()
                .is_some_and(|l| l.vsl.limit == h.vsl.limit && h.at_m - l.at_m <= VSL_GAP_M)
            {
                continue;
            }
            out.push(h);
        }
        out
    }
}

async fn fetch(http: &reqwest::Client, key: &str) -> Result<Vec<VslLink>, String> {
    let response = http
        .get(VSL_URL)
        .query(&[("apiKey", key), ("getType", "json")])
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("호출 실패: {}", e.without_url()))?;
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("본문 읽기 실패: {}", e.without_url()))?;
    parse(&body)
}

/// interval마다 가변속도제한을 받아 배율과 store를 바꾼다. 실패가 STALE_POLLS 번 이어지면 배율을 되돌리고 store를 비운다
pub async fn run(
    http: reqwest::Client,
    key: String,
    db: Arc<RouteDb>,
    store: Arc<VslStore>,
    updates: UpdateSender,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    let mut fresh = Freshness::new();
    let mut previous: HashSet<u64> = HashSet::new();
    loop {
        ticker.tick().await;
        let started = Instant::now();
        match fetch(&http, &key).await {
            Ok(links) => {
                fresh.succeeded(Instant::now());
                let batch = to_updates(&links, &previous, &db);
                let lowered: Vec<VslLink> = links
                    .into_iter()
                    .filter(VslLink::lowered)
                    .map(|mut v| {
                        if let Some(e) = db.edge_of_link(v.link) {
                            v.points = db.edge_points(e as usize);
                        }
                        v
                    })
                    .collect();
                println!(
                    "가변속도제한 링크 {} 제한속도 낮아짐 {} 배율 {} ({:.1}초)",
                    batch.len(),
                    lowered.len(),
                    batch.iter().filter(|u| u.vsl_factor != Some(1.0)).count(),
                    started.elapsed().as_secs_f64()
                );
                previous = lowered.iter().map(|v| v.link).collect();
                store.replace(lowered);
                if updates.send(Batch::Updates(batch)).await.is_err() {
                    eprintln!("적용 태스크가 없어 가변속도제한을 버림");
                }
            }
            Err(e) => {
                eprintln!("가변속도제한 실패: {e}");
                if fresh.failed(Instant::now(), interval) {
                    println!("가변속도제한을 오래 받지 못해 배율을 되돌림");
                    previous.clear();
                    store.replace(Vec::new());
                    let _ = updates.send(Batch::Reset(FactorKind::Vsl)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_keeps_lowest_limit_and_fixes_swapped_coords() {
        let body = json!({"header": {"resultCode": 0, "resultMsg": "SUCCESS"}, "body": {"items": [
            {"linkId": "100", "limitSpeed": "110", "defLmtSpeed": "110", "coordX": "37.4", "coordY": "127.1"},
            {"linkId": "100", "limitSpeed": "80", "defLmtSpeed": "110", "coordX": "127.2", "coordY": "37.5"},
            {"linkId": "200", "limitSpeed": "0", "defLmtSpeed": "100", "coordX": "127.0", "coordY": "36.0"},
            {"linkId": "x", "limitSpeed": "60", "defLmtSpeed": "100", "coordX": "127.0", "coordY": "36.0"}
        ]}});
        let links = parse(&body).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].limit, 80);
        assert!(links[0].lowered());
        assert_eq!((links[0].lat, links[0].lon), (37.5, 127.2));
    }

    #[test]
    fn parse_error_code() {
        let body = json!({"header": {"resultCode": 4002, "resultMsg": "필수 입력값"}, "body": ""});
        assert!(parse(&body).is_err());
    }
}
