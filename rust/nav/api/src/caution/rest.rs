//! 고속도로 휴게소. scripts/caution/build_rest_areas.py가 한국도로공사 API로 만든 휴게소.csv를 읽는다.
//! 우측통행이라 휴게소는 진행 방향 오른쪽에 있으므로, 경로의 고속국도 구간에서 휴게소가 진행 방향 오른쪽 REST_MATCH_M 안이면 안내한다.
//! 반대 방향 휴게소는 왼쪽에 있어 빠진다

use std::io;
use std::path::Path;

use rstar::{AABB, RTree, RTreeObject};
use serde::Deserialize;

use crate::accident::index::cumulative_m;

/// 휴게소가 경로에서 이 거리 안이면 안내한다(m). 휴게소 좌표는 본선에서 수백 m 떨어진 주차장·건물 위치이다
pub const REST_MATCH_M: f64 = 400.0;

#[derive(Deserialize)]
struct Row {
    code: String,
    name: String,
    route: String,
    lat: f64,
    lon: f64,
    gas: String,
    convenience: String,
}

/// 휴게소 하나
#[derive(Clone, Debug, PartialEq)]
pub struct RestArea {
    pub code: String,
    pub name: String,
    pub route: String,
    pub lat: f64,
    pub lon: f64,
    /// 주유소가 있음
    pub gas: bool,
    /// 편의시설. 예: 수유실·농산물판매장
    pub convenience: String,
}

/// 경로 옆 휴게소
#[derive(Clone, Debug, PartialEq)]
pub struct RestHit {
    pub at_m: f64,
    pub rest: RestArea,
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

/// 휴게소 색인
pub struct RestAreaIndex {
    items: Vec<RestArea>,
    tree: RTree<Located>,
}

impl RestAreaIndex {
    /// 휴게소.csv(code, name, route, lat, lon, gas, convenience)를 읽는다
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut reader = csv::Reader::from_path(path)?;
        let mut items = Vec::new();
        for row in reader.deserialize::<Row>() {
            let row = row.map_err(io::Error::other)?;
            items.push(RestArea {
                code: row.code,
                name: row.name,
                route: row.route,
                lat: row.lat,
                lon: row.lon,
                gas: row.gas == "Y",
                convenience: row.convenience,
            });
        }
        Ok(Self::build(items))
    }

    pub fn build(items: Vec<RestArea>) -> Self {
        let tree = RTree::bulk_load(
            items
                .iter()
                .enumerate()
                .map(|(index, r)| Located {
                    index,
                    point: [r.lon, r.lat],
                })
                .collect(),
        );
        Self { items, tree }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 경로 좌표 열의 expressway 구간(출발점부터의 거리)에서 진행 방향 오른쪽 REST_MATCH_M 안의 휴게소. 거리 순서이다
    pub fn hits(
        &self,
        geometry: &[(f64, f64)],
        distance_m: f64,
        expressway: &[(f64, f64)],
    ) -> Vec<RestHit> {
        let cumulative = cumulative_m(geometry, distance_m);
        // 휴게소마다 가장 가까운 선분의 (거리, 경로 위 거리, 오른쪽 여부)
        let mut best: Vec<Option<(f64, f64, bool)>> = vec![None; self.items.len()];
        let (pad_lat, pad_lon) = (REST_MATCH_M / 110_950.0, REST_MATCH_M / 88_000.0);
        for (i, pair) in geometry.windows(2).enumerate() {
            let (p, q) = (pair[0], pair[1]);
            let envelope = AABB::from_corners(
                [p.1.min(q.1) - pad_lon, p.0.min(q.0) - pad_lat],
                [p.1.max(q.1) + pad_lon, p.0.max(q.0) + pad_lat],
            );
            for c in self.tree.locate_in_envelope_intersecting(envelope) {
                let r = &self.items[c.index];
                let cos = r.lat.to_radians().cos();
                // 휴게소를 원점으로 한 m 좌표(x 동쪽, y 북쪽)
                let xy =
                    |v: (f64, f64)| ((v.1 - r.lon) * 111_320.0 * cos, (v.0 - r.lat) * 110_950.0);
                let (a, b) = (xy(p), xy(q));
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len2 = dx * dx + dy * dy;
                if len2 == 0.0 {
                    continue;
                }
                let t = (-(a.0 * dx + a.1 * dy) / len2).clamp(0.0, 1.0);
                let d = (a.0 + t * dx).hypot(a.1 + t * dy);
                if d > REST_MATCH_M {
                    continue;
                }
                // 진행 방향 (dx, dy)에서 휴게소(원점) 쪽 벡터 (−a)의 외적이 음수이면 오른쪽이다
                let right = dx * (-a.1) - dy * (-a.0) < 0.0;
                let at = cumulative[i] + t * (cumulative[i + 1] - cumulative[i]);
                if best[c.index].is_none_or(|(old, _, _)| d < old) {
                    best[c.index] = Some((d, at, right));
                }
            }
        }
        let on_expressway = |at: f64| expressway.iter().any(|&(s, e)| s <= at && at <= e);
        let mut out: Vec<RestHit> = best
            .iter()
            .enumerate()
            .filter_map(|(i, b)| match *b {
                Some((_, at_m, true)) if on_expressway(at_m) => Some(RestHit {
                    at_m,
                    rest: self.items[i].clone(),
                }),
                _ => None,
            })
            .collect();
        out.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest(name: &str, lat: f64, lon: f64) -> RestArea {
        RestArea {
            code: name.to_string(),
            name: name.to_string(),
            route: "경부선".to_string(),
            lat,
            lon,
            gas: true,
            convenience: String::new(),
        }
    }

    #[test]
    fn right_side_only() {
        // 동쪽으로 가는 2km 고속도로. 남쪽(오른쪽) 100m의 A만 안내하고 북쪽(왼쪽) 100m의 B와 남쪽 600m의 C는 뺀다
        let east = |m: f64| 127.0 + m / (111_320.0 * 37.5f64.to_radians().cos());
        let index = RestAreaIndex::build(vec![
            rest("A", 37.5 - 100.0 / 110_950.0, east(1_000.0)),
            rest("B", 37.5 + 100.0 / 110_950.0, east(1_200.0)),
            rest("C", 37.5 - 600.0 / 110_950.0, east(1_500.0)),
        ]);
        let geometry = vec![(37.5, 127.0), (37.5, east(2_000.0))];
        let hits = index.hits(&geometry, 2_000.0, &[(0.0, 2_000.0)]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rest.name, "A");
        // 고속국도 구간이 아니면 안내하지 않는다
        assert!(index.hits(&geometry, 2_000.0, &[]).is_empty());
        // 서쪽으로 가면 B가 오른쪽이다
        let back = vec![(37.5, east(2_000.0)), (37.5, 127.0)];
        let hits = index.hits(&back, 2_000.0, &[(0.0, 2_000.0)]);
        assert_eq!(
            hits.iter()
                .map(|h| h.rest.name.as_str())
                .collect::<Vec<_>>(),
            vec!["B"]
        );
    }
}
