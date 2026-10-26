//! 재난상황정보(국가교통정보센터 `/disasterInfo`). 침수·하천범람·땅꺼짐·화재를 주기적으로 받아 두고, 종료 일시가 지나지 않은 재난 중
//! 경로가 위치(점·선·영역 꼭짓점)에서 DISASTER_MATCH_M 안을 지나거나, 영역 안을 지나거나, 재난 링크를 지나면 경고한다

use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use super::construction::ymd_from_days;
use super::{nearest_on_route, result_code, route_position};

const DISASTER_URL: &str = "https://openapi.its.go.kr:9443/disasterInfo";
/// 재난 위치가 경로에서 이 거리 안이면 경고한다(m)
pub const DISASTER_MATCH_M: f64 = 100.0;
/// 발생 일시가 오늘부터 이 일수 안인 재난을 받는다
pub const DISASTER_DAYS: i64 = 30;
/// 이보다 오래된 자료는 쓰지 않는다
pub const DISASTER_MAX_AGE: Duration = Duration::from_secs(3 * 60 * 60);

/// 응답 항목 하나. 값은 모두 문자열이다
#[derive(Debug, Default, Deserialize)]
struct Item {
    #[serde(rename = "eventId", default)]
    id: String,
    #[serde(rename = "eventType", default)]
    kind: String,
    #[serde(rename = "endDate", default)]
    end: String,
    #[serde(rename = "socName", default)]
    place: String,
    #[serde(rename = "locationInfo", default)]
    location: String,
    #[serde(rename = "locationInfoType", default)]
    shape: String,
    #[serde(rename = "linkId", default)]
    links: String,
    #[serde(default)]
    message: String,
}

/// 재난 하나
#[derive(Clone, Debug, PartialEq)]
pub struct Disaster {
    pub id: String,
    /// 유형 코드 D03 침수, D04 하천범람, D06 땅꺼짐, D07 화재
    pub kind: String,
    /// 종료 일시 YYYYMMDDHHMM. 없으면 빈 문자열
    pub end: String,
    /// 피해시설물 명
    pub place: String,
    pub message: String,
    /// 위치 좌표 (위도, 경도). 영역이면 꼭짓점이다
    pub points: Vec<(f64, f64)>,
    /// 영역이면 true
    pub area: bool,
    pub links: Vec<u64>,
}

/// 경로가 지나는 재난
#[derive(Clone, Debug, PartialEq)]
pub struct DisasterHit {
    pub at_m: f64,
    pub disaster: Disaster,
}

/// 유형 이름. 국가교통정보센터 재난상황정보 요청변수 eventType 설명이다
pub fn kind_text(code: &str) -> &'static str {
    match code {
        "D03" => "침수",
        "D04" => "하천범람",
        "D06" => "땅꺼짐",
        "D07" => "화재",
        _ => "재난",
    }
}

/// 한국 시간으로 1970-01-01부터 지난 초
fn kst_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        + 9 * 3600
}

/// 지금의 한국 시간 YYYYMMDDHHMM
pub fn now_kst() -> String {
    let secs = kst_secs();
    let minutes = secs % 86_400 / 60;
    format!(
        "{}{:02}{:02}",
        ymd_from_days((secs / 86_400) as i64),
        minutes / 60,
        minutes % 60
    )
}

/// WKT 또는 "X Y,X Y" 위치에서 좌표 (위도, 경도)를 꺼낸다. MULTIPOLYGON은 모든 꼭짓점이다
fn points_of(location: &str) -> Vec<(f64, f64)> {
    location
        .split([',', '(', ')'])
        .filter_map(|pair| {
            let mut it = pair
                .split_whitespace()
                .filter_map(|v| v.parse::<f64>().ok());
            let (x, y) = (it.next()?, it.next()?);
            Some((y, x))
        })
        .collect()
}

/// 점 p가 꼭짓점 열 ring이 이루는 다각형 안이면 true
fn inside(ring: &[(f64, f64)], p: (f64, f64)) -> bool {
    let mut odd = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a.0 > p.0) != (b.0 > p.0) && p.1 < (b.1 - a.1) * (p.0 - a.0) / (b.0 - a.0) + a.1 {
            odd = !odd;
        }
        j = i;
    }
    odd
}

struct Snapshot {
    disasters: Vec<Disaster>,
    fetched: Instant,
}

/// 마지막으로 받은 재난. 폴링 태스크가 바꾸고 경로 응답이 읽는다
pub struct DisasterStore {
    snapshot: RwLock<Option<Snapshot>>,
}

impl Default for DisasterStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DisasterStore {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(None),
        }
    }

    pub fn replace(&self, disasters: Vec<Disaster>) {
        *self.snapshot.write().expect("재난 잠금 실패") = Some(Snapshot {
            disasters,
            fetched: Instant::now(),
        });
    }

    /// 시각 now(YYYYMMDDHHMM)에 끝나지 않았고 경로가 지나는 재난. 거리 순서이다
    pub fn hits(
        &self,
        links: &[u64],
        geometry: &[(f64, f64)],
        distance_m: f64,
        now: &str,
    ) -> Vec<DisasterHit> {
        let guard = self.snapshot.read().expect("재난 잠금 실패");
        let Some(snap) = guard
            .as_ref()
            .filter(|s| s.fetched.elapsed() <= DISASTER_MAX_AGE)
        else {
            return Vec::new();
        };
        let on_route: HashSet<u64> = links.iter().copied().collect();
        let position = route_position(geometry, distance_m);
        let mut out = Vec::new();
        for d in &snap.disasters {
            if !d.end.is_empty() && d.end.as_str() < now {
                continue;
            }
            let nearest = d
                .points
                .iter()
                .filter_map(|&(lat, lon)| nearest_on_route(geometry, &position, lat, lon))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let at_m = match nearest {
                Some((dist, at)) if dist <= DISASTER_MATCH_M => Some(at),
                _ if d.area && d.points.len() >= 3 => geometry
                    .iter()
                    .position(|&p| inside(&d.points, p))
                    .map(|i| position[i]),
                _ if d.links.iter().any(|l| on_route.contains(l)) => nearest.map(|(_, at)| at),
                _ => None,
            };
            if let Some(at_m) = at_m {
                out.push(DisasterHit {
                    at_m,
                    disaster: d.clone(),
                });
            }
        }
        out.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
        out
    }
}

/// 응답 본문을 재난 목록으로 바꾼다
pub fn parse(body: &Value) -> Result<Vec<Disaster>, String> {
    result_code(body, "재난")?;
    let items = body["body"]["items"].clone();
    if items.is_null() || items.as_str() == Some("") {
        return Ok(Vec::new());
    }
    let items: Vec<Item> =
        serde_json::from_value(items).map_err(|e| format!("재난 항목 해석 실패: {e}"))?;
    Ok(items
        .into_iter()
        .map(|i| Disaster {
            id: i.id.trim().to_string(),
            kind: i.kind.trim().to_string(),
            end: i.end.trim().chars().take(12).collect(),
            place: i.place.trim().to_string(),
            message: i.message.split_whitespace().collect::<Vec<_>>().join(" "),
            points: points_of(&i.location),
            area: i.shape.trim() == "3",
            links: i
                .links
                .split(',')
                .filter_map(|v| v.trim().parse().ok())
                .collect(),
        })
        .collect())
}

/// interval마다 최근 DISASTER_DAYS일에 발생한 재난을 받아 store를 바꾼다. 실패하면 이전 자료를 그대로 둔다
pub async fn run(
    http: reqwest::Client,
    key: String,
    store: Arc<DisasterStore>,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let started = Instant::now();
        let days = (kst_secs() / 86_400) as i64;
        let (from, to) = (
            ymd_from_days(days - DISASTER_DAYS).to_string(),
            ymd_from_days(days).to_string(),
        );
        let result = async {
            let response = http
                .get(DISASTER_URL)
                .query(&[
                    ("apiKey", key.as_str()),
                    ("category", "D"),
                    ("eventType", "all"),
                    ("startDate", from.as_str()),
                    ("endDate", to.as_str()),
                    ("getType", "json"),
                ])
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
        .await;
        match result {
            Ok(disasters) => {
                let now = now_kst();
                let active = disasters
                    .iter()
                    .filter(|d| d.end.is_empty() || d.end >= now)
                    .count();
                println!(
                    "재난상황정보 {}건, 끝나지 않은 재난 {active}건 ({:.1}초)",
                    disasters.len(),
                    started.elapsed().as_secs_f64()
                );
                store.replace(disasters);
            }
            Err(e) => eprintln!("재난상황정보 실패: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_match() {
        let body: Value = serde_json::from_str(
            r#"{"header":{"resultCode":0,"resultMsg":"SUCCESS"},"body":{"totalCount":"3","items":[
            {"eventId":"A","eventType":"D03","endDate":"","socName":"가 지하차도","locationInfoType":"3","locationInfo":"POLYGON((127.004 37.499, 127.006 37.499, 127.006 37.501, 127.004 37.501, 127.004 37.499))","linkId":"","message":"지하차도 침수"},
            {"eventId":"B","eventType":"D07","endDate":"202610010000","socName":"","locationInfoType":"1","locationInfo":"127.005 37.5001","linkId":"","message":""},
            {"eventId":"C","eventType":"D07","endDate":"","socName":"","locationInfoType":"1","locationInfo":"127.005 37.52","linkId":"","message":""}]}}"#,
        )
        .unwrap();
        let disasters = parse(&body).unwrap();
        assert_eq!(disasters.len(), 3);
        assert!(disasters[0].area);
        assert_eq!(disasters[0].points.len(), 5);
        let store = DisasterStore::new();
        store.replace(disasters);
        // 꼭짓점이 경로에서 111m 떨어진 A는 경로가 영역 안을 지나고, B는 2026-10-01에 끝났고, C는 2km 넘게 떨어져 있다
        let geometry = [(37.5, 127.0), (37.5, 127.005), (37.5, 127.01)];
        let hits = store.hits(&[], &geometry, 880.0, "202610261200");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].disaster.id, "A");
        assert_eq!(kind_text(&hits[0].disaster.kind), "침수");
        assert_eq!(now_kst().len(), 12);
    }
}
