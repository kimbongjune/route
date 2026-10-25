//! 시작점과 끝점이 있는 주의 구간(상습 결빙구간, 안개 잦은 구간). scripts/caution의 스크립트가 만든 CSV를 읽는다.
//! 경로가 두 점 모두 판정 거리(결빙 ICING_MATCH_M, 안개 FOG_MATCH_M) 안을 지나고 경로 위 두 점 사이 거리가 직선거리의 0.5배 이상,
//! 직선거리 × 2 + 300m 이하이면 그 구간을 지나는 것으로 본다.
//! 원본의 총길이는 잘못 들어간 값이 있어 쓰지 않는다. 같은 id의 구간(양방향 본선 등)은 하나만 남긴다

use std::io;
use std::path::Path;

use rstar::{AABB, RTree, RTreeObject};
use serde::{Deserialize, Serialize};

use crate::accident::index::cumulative_m;

/// 결빙구간 시작점·끝점이 경로에서 이 거리 안이면 경로가 그 점을 지난다고 본다(m)
pub const ICING_MATCH_M: f64 = 50.0;
/// 안개 구간(고속국도 본선) 판정 거리(m). IC에서 진입로가 여러 갈래라 본선 합류 지점이 경로마다 다르다
pub const FOG_MATCH_M: f64 = 500.0;
/// 경로 위 두 점 사이 거리 하한. 직선거리의 배수이며 구간을 가로질러 지나는 경로를 뺀다
const MIN_ALONG_FACTOR: f64 = 0.5;
/// 경로 위 두 점 사이 거리 상한. 직선거리의 배수와 더하는 거리(m)
const DETOUR_FACTOR: f64 = 2.0;
const DETOUR_EXTRA_M: f64 = 300.0;

/// 구간 CSV 한 행
#[derive(Deserialize)]
struct Row {
    id: String,
    kind: String,
    region: String,
    road: String,
    start_lat: f64,
    start_lon: f64,
    end_lat: f64,
    end_lon: f64,
}

/// 주의 구간 하나
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CautionZone {
    pub id: String,
    /// 도로 분류. 지방도, 시도, 군도, 일반국도, 고속국도 등
    pub kind: String,
    pub region: String,
    pub road: String,
    pub start: (f64, f64),
    pub end: (f64, f64),
}

/// 경로가 지나는 주의 구간. 출발점부터의 시작·끝 거리이다
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CautionSection {
    pub from_m: f64,
    pub to_m: f64,
    pub zone: CautionZone,
}

/// R-tree에 넣는 구간 시작점 또는 끝점. 좌표는 [경도, 위도]
struct EndPoint {
    zone: usize,
    is_end: bool,
    point: [f64; 2],
}

impl RTreeObject for EndPoint {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.point)
    }
}

/// 주의 구간 색인
pub struct SectionIndex {
    zones: Vec<CautionZone>,
    tree: RTree<EndPoint>,
    /// 판정 거리(m)
    match_m: f64,
}

/// 위경도 a에서 선분 p-q까지의 거리(m)와 p에서 투영점까지의 비율(0~1)
fn point_segment(a: (f64, f64), p: (f64, f64), q: (f64, f64)) -> (f64, f64) {
    let cos = a.0.to_radians().cos();
    let xy = |c: (f64, f64)| ((c.1 - a.1) * 111_320.0 * cos, (c.0 - a.0) * 110_950.0);
    let (p, q) = (xy(p), xy(q));
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (-(p.0 * dx + p.1 * dy) / len2).clamp(0.0, 1.0)
    };
    ((p.0 + t * dx).hypot(p.1 + t * dy), t)
}

/// 두 위경도의 직선거리(m)
fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    let cos = ((a.0 + b.0) / 2.0).to_radians().cos();
    ((b.1 - a.1) * 111_320.0 * cos).hypot((b.0 - a.0) * 110_950.0)
}

impl SectionIndex {
    /// 구간 CSV(id, kind, region, road, length_m, start_lat, start_lon, end_lat, end_lon)를 읽는다
    pub fn load(path: &Path, match_m: f64) -> io::Result<Self> {
        let mut reader = csv::Reader::from_path(path)?;
        let mut zones = Vec::new();
        for row in reader.deserialize::<Row>() {
            let row = row.map_err(io::Error::other)?;
            zones.push(CautionZone {
                id: row.id,
                kind: row.kind,
                region: row.region,
                road: row.road,
                start: (row.start_lat, row.start_lon),
                end: (row.end_lat, row.end_lon),
            });
        }
        Ok(Self::build(zones, match_m))
    }

    /// 구간 목록으로 색인을 만든다
    pub fn build(zones: Vec<CautionZone>, match_m: f64) -> Self {
        let mut points = Vec::with_capacity(zones.len() * 2);
        for (i, z) in zones.iter().enumerate() {
            for (is_end, c) in [(false, z.start), (true, z.end)] {
                points.push(EndPoint {
                    zone: i,
                    is_end,
                    point: [c.1, c.0],
                });
            }
        }
        Self {
            zones,
            tree: RTree::bulk_load(points),
            match_m,
        }
    }

    pub fn len(&self) -> usize {
        self.zones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.zones.is_empty()
    }

    /// 경로 좌표 열 (위도, 경도)이 지나는 구간. 거리는 좌표 열의 누적 길이를 경로 거리에 맞춘 값이다
    pub fn sections(&self, geometry: &[(f64, f64)], distance_m: f64) -> Vec<CautionSection> {
        let cumulative = cumulative_m(geometry, distance_m);
        // 구간마다 시작점과 끝점에 가장 가까운 경로 위치 (거리, 경로 위 거리)
        let mut near: Vec<[Option<(f64, f64)>; 2]> = vec![[None, None]; self.zones.len()];
        let (pad_lat, pad_lon) = (self.match_m / 110_950.0, self.match_m / 88_000.0);
        for (i, pair) in geometry.windows(2).enumerate() {
            let (p, q) = (pair[0], pair[1]);
            let envelope = AABB::from_corners(
                [p.1.min(q.1) - pad_lon, p.0.min(q.0) - pad_lat],
                [p.1.max(q.1) + pad_lon, p.0.max(q.0) + pad_lat],
            );
            for ep in self.tree.locate_in_envelope_intersecting(envelope) {
                let (d, t) = point_segment((ep.point[1], ep.point[0]), p, q);
                if d > self.match_m {
                    continue;
                }
                let at = cumulative[i] + t * (cumulative[i + 1] - cumulative[i]);
                let slot = &mut near[ep.zone][usize::from(ep.is_end)];
                if slot.is_none_or(|(best, _)| d < best) {
                    *slot = Some((d, at));
                }
            }
        }
        let mut out: Vec<CautionSection> = Vec::new();
        for (zone, [start, end]) in self.zones.iter().zip(near) {
            let (Some((_, a)), Some((_, b))) = (start, end) else {
                continue;
            };
            let (from_m, to_m) = (a.min(b), a.max(b));
            let straight = distance(zone.start, zone.end);
            let along = to_m - from_m;
            if along > straight * DETOUR_FACTOR + DETOUR_EXTRA_M
                || along < straight * MIN_ALONG_FACTOR
            {
                continue;
            }
            if out.iter().any(|o| o.zone.id == zone.id) {
                continue;
            }
            out.push(CautionSection {
                from_m,
                to_m,
                zone: zone.clone(),
            });
        }
        out.sort_by(|x, y| x.from_m.total_cmp(&y.from_m));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(id: &str, start: (f64, f64), end: (f64, f64)) -> CautionZone {
        CautionZone {
            id: id.to_string(),
            kind: "지방도".to_string(),
            region: String::new(),
            road: "지방도1호선".to_string(),
            start,
            end,
        }
    }

    #[test]
    fn route_through_icing_zone() {
        // 동쪽으로 2km 가는 경로. A는 경로 위 500m~1,500m, B는 끝점이 경로에서 200m 떨어져 있고, C는 반대 방향으로 놓인 구간이다
        let geometry = vec![(37.5, 127.0), (37.5, 127.0226)];
        let east = |m: f64| 127.0 + m / (111_320.0 * 37.5f64.to_radians().cos());
        let index = SectionIndex::build(
            vec![
                zone("A", (37.5, east(500.0)), (37.5, east(1_500.0))),
                zone(
                    "B",
                    (37.5, east(300.0)),
                    (37.5 + 200.0 / 110_950.0, east(800.0)),
                ),
                zone(
                    "C",
                    (37.5 + 10.0 / 110_950.0, east(1_800.0)),
                    (37.5, east(1_200.0)),
                ),
                // 경로를 가로지르는 구간. 두 점이 경로 위 같은 자리로 잡혀 뺀다
                zone(
                    "D",
                    (37.5 + 30.0 / 110_950.0, east(1_000.0)),
                    (37.5 - 30.0 / 110_950.0, east(1_000.0)),
                ),
            ],
            ICING_MATCH_M,
        );
        let hits = index.sections(&geometry, 2_000.0);
        let ids: Vec<&str> = hits.iter().map(|h| h.zone.id.as_str()).collect();
        assert_eq!(ids, vec!["A", "C"]);
        assert!((hits[0].from_m - 500.0).abs() < 5.0, "{}", hits[0].from_m);
        assert!((hits[0].to_m - 1_500.0).abs() < 5.0, "{}", hits[0].to_m);
        assert!((hits[1].from_m - 1_200.0).abs() < 5.0);
    }
}
