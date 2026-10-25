//! 도로위험상황예보(경찰청 도시교통정보센터 UTIC `getRoadAccJson.do`). 매시 40분에 갱신되는 위험 지점을 주기적으로 받아 두고,
//! 경로가 지점에서 RISK_MATCH_M 안을 지나면 경고한다. 받은 지 RISK_MAX_AGE가 지난 자료는 쓰지 않는다

use std::sync::RwLock;
use std::time::{Duration, Instant};

use rstar::{AABB, RTree, RTreeObject};
use serde::Deserialize;

use crate::accident::index::cumulative_m;

const RISK_URL: &str = "http://www.utic.go.kr/guide/getRoadAccJson.do";
/// 위험 지점이 경로에서 이 거리 안이면 경고한다(m)
pub const RISK_MATCH_M: f64 = 50.0;
/// 이보다 오래된 자료는 쓰지 않는다. 갱신 주기 1시간의 두 배이다
pub const RISK_MAX_AGE: Duration = Duration::from_secs(2 * 60 * 60);

/// 응답 항목 하나
#[derive(Deserialize)]
struct Item {
    #[serde(rename = "SAFEDATA_ID")]
    id: String,
    #[serde(rename = "S_CD", default)]
    element: String,
    #[serde(rename = "GRADE")]
    grade: String,
    #[serde(rename = "LOCATION_X")]
    lon: f64,
    #[serde(rename = "LOCATION_Y")]
    lat: f64,
    #[serde(rename = "ADDRESS_JIBUN", default)]
    address: String,
}

#[derive(Deserialize)]
struct Body {
    #[serde(rename = "resultCode")]
    result_code: String,
    #[serde(default)]
    items: Vec<Item>,
}

/// 위험 지점 하나
#[derive(Clone, Debug, PartialEq)]
pub struct RiskPoint {
    pub id: String,
    /// 위험요소 코드 S_CD
    pub element: String,
    /// 위험요소 등급 A~G
    pub grade: char,
    pub lat: f64,
    pub lon: f64,
    pub address: String,
}

/// 경로가 지나는 위험 지점
#[derive(Clone, Debug, PartialEq)]
pub struct RiskHit {
    pub at_m: f64,
    pub point: RiskPoint,
}

/// 위험요소 등급. UTIC 코드정의서 roadAcc_Code.xlsx의 GRADE 시트이다.
/// 레퍼런스 웹 페이지(utisRefRoadAcc.do)의 A~L 표와 다르며 코드정의서를 따른다
pub fn grade_text(grade: char) -> Option<&'static str> {
    Some(match grade {
        'A' => "경미(비)",
        'B' => "주의(비)",
        'C' => "위험(비)",
        'D' => "주의(비/눈)",
        'E' => "위험(눈)",
        'F' => "주의(눈)",
        'G' => "위험(눈)",
        _ => return None,
    })
}

/// 위험요소 이름. roadAcc_Code.xlsx의 S_CD 시트이다
pub fn element_text(code: &str) -> Option<&'static str> {
    Some(match code {
        "SF20202" => "안전거리미확보",
        "SF20204" => "과속",
        "SF20302" => "급커브(굽은도로)",
        "SF20304" => "급경사(내리막)",
        "SF20306" => "미끄러운도로",
        "SF20309" => "침수",
        "SF20310" => "기상-비",
        "SF20311" => "기상-눈",
        "SF20313" => "기상-결빙",
        _ => return None,
    })
}

struct Located {
    index: usize,
    point: [f64; 2],
}

impl RTreeObject for Located {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.point)
    }
}

struct Snapshot {
    points: Vec<RiskPoint>,
    tree: RTree<Located>,
    fetched: Instant,
}

/// 마지막으로 받은 위험 지점. 폴링 태스크가 바꾸고 경로 응답이 읽는다
pub struct RiskStore {
    snapshot: RwLock<Option<Snapshot>>,
}

impl Default for RiskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RiskStore {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(None),
        }
    }

    /// 위험 지점 목록을 바꾼다
    pub fn replace(&self, points: Vec<RiskPoint>) {
        let tree = RTree::bulk_load(
            points
                .iter()
                .enumerate()
                .map(|(index, p)| Located {
                    index,
                    point: [p.lon, p.lat],
                })
                .collect(),
        );
        *self.snapshot.write().expect("위험 지점 잠금 실패") = Some(Snapshot {
            points,
            tree,
            fetched: Instant::now(),
        });
    }

    /// 지금 가진 지점 수. 없거나 오래되었으면 0
    pub fn len(&self) -> usize {
        let guard = self.snapshot.read().expect("위험 지점 잠금 실패");
        guard
            .as_ref()
            .filter(|s| s.fetched.elapsed() <= RISK_MAX_AGE)
            .map_or(0, |s| s.points.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 경로 좌표 열이 RISK_MATCH_M 안을 지나는 위험 지점. 지점마다 가장 가까운 경로 위치이며 거리 순서이다
    pub fn hits(&self, geometry: &[(f64, f64)], distance_m: f64) -> Vec<RiskHit> {
        let guard = self.snapshot.read().expect("위험 지점 잠금 실패");
        let Some(snap) = guard
            .as_ref()
            .filter(|s| s.fetched.elapsed() <= RISK_MAX_AGE)
        else {
            return Vec::new();
        };
        let cumulative = cumulative_m(geometry, distance_m);
        let mut best: Vec<Option<(f64, f64)>> = vec![None; snap.points.len()];
        let (pad_lat, pad_lon) = (RISK_MATCH_M / 110_950.0, RISK_MATCH_M / 88_000.0);
        for (i, pair) in geometry.windows(2).enumerate() {
            let (p, q) = (pair[0], pair[1]);
            let envelope = AABB::from_corners(
                [p.1.min(q.1) - pad_lon, p.0.min(q.0) - pad_lat],
                [p.1.max(q.1) + pad_lon, p.0.max(q.0) + pad_lat],
            );
            for c in snap.tree.locate_in_envelope_intersecting(envelope) {
                let center = (c.point[1], c.point[0]);
                let cos = center.0.to_radians().cos();
                let xy = |v: (f64, f64)| {
                    (
                        (v.1 - center.1) * 111_320.0 * cos,
                        (v.0 - center.0) * 110_950.0,
                    )
                };
                let (a, b) = (xy(p), xy(q));
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len2 = dx * dx + dy * dy;
                let t = if len2 == 0.0 {
                    0.0
                } else {
                    (-(a.0 * dx + a.1 * dy) / len2).clamp(0.0, 1.0)
                };
                let d = (a.0 + t * dx).hypot(a.1 + t * dy);
                if d > RISK_MATCH_M {
                    continue;
                }
                let at = cumulative[i] + t * (cumulative[i + 1] - cumulative[i]);
                if best[c.index].is_none_or(|(old, _)| d < old) {
                    best[c.index] = Some((d, at));
                }
            }
        }
        let mut out: Vec<RiskHit> = best
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                b.map(|(_, at_m)| RiskHit {
                    at_m,
                    point: snap.points[i].clone(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
        out
    }
}

/// 응답 본문을 위험 지점 목록으로 바꾼다. resultCode가 00이 아니면 오류이다
pub fn parse(body: &[u8]) -> Result<Vec<RiskPoint>, String> {
    let body: Body = serde_json::from_slice(body).map_err(|e| format!("응답 해석 실패: {e}"))?;
    if body.result_code != "00" {
        return Err(format!("resultCode {}", body.result_code));
    }
    Ok(body
        .items
        .into_iter()
        .filter_map(|i| {
            let grade = i.grade.trim().chars().next()?;
            Some(RiskPoint {
                id: i.id,
                element: i.element.trim().to_string(),
                grade,
                lat: i.lat,
                lon: i.lon,
                address: i.address.trim().to_string(),
            })
        })
        .collect())
}

/// interval마다 도로위험상황예보를 받아 store를 바꾼다. 실패하면 이전 자료를 그대로 둔다
pub async fn run(
    http: reqwest::Client,
    key: String,
    store: std::sync::Arc<RiskStore>,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let started = Instant::now();
        let result = async {
            let response = http
                .get(RISK_URL)
                .query(&[("key", key.as_str())])
                .timeout(Duration::from_secs(60))
                .send()
                .await
                .map_err(|e| format!("호출 실패: {}", e.without_url()))?;
            let bytes = response
                .bytes()
                .await
                .map_err(|e| format!("본문 읽기 실패: {}", e.without_url()))?;
            parse(&bytes)
        }
        .await;
        match result {
            Ok(points) => {
                println!(
                    "도로위험상황예보 {}곳 ({:.1}초)",
                    points.len(),
                    started.elapsed().as_secs_f64()
                );
                store.replace(points);
            }
            Err(e) => eprintln!("도로위험상황예보 실패: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_match() {
        let body = r#"{"resultCode":"00","totalCount":"2","items":[
            {"SAFEDATA_ID":"L1","S_CD":"SF20302","GRADE":"B","LOCATION_X":127.0045,"LOCATION_Y":37.5003,"ADDRESS_JIBUN":"가동 1"},
            {"SAFEDATA_ID":"L2","S_CD":"SF20304","GRADE":"C","LOCATION_X":127.0045,"LOCATION_Y":37.5100,"ADDRESS_JIBUN":"나동 2"}]}"#;
        let points = parse(body.as_bytes()).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].grade, 'B');
        assert!(parse(br#"{"resultCode":"99","items":[]}"#).is_err());
        let store = RiskStore::new();
        assert!(
            store
                .hits(&[(37.5, 127.0), (37.5, 127.01)], 880.0)
                .is_empty()
        );
        store.replace(points);
        // 동쪽으로 가는 경로에서 L1은 북쪽 33m, L2는 1km 넘게 떨어져 있다
        let hits = store.hits(&[(37.5, 127.0), (37.5, 127.01)], 880.0);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].point.id, "L1");
        assert!((hits[0].at_m - 396.0).abs() < 5.0, "{}", hits[0].at_m);
        assert_eq!(grade_text('B'), Some("주의(비)"));
        assert_eq!(
            element_text(&hits[0].point.element),
            Some("급커브(굽은도로)")
        );
    }
}
