//! 공사 지점. scripts/caution/build_construction.py가 UTIC TCS 공사정보에서 주소로 위치를 찾은 공사만 모은 공사구간.csv를 읽는다.
//! 요청 날짜(한국 시간)가 공사 시작일과 종료일 사이이고 경로가 지점에서 CONSTRUCTION_MATCH_M 안을 지나면 경고한다

use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rstar::{AABB, RTree, RTreeObject};
use serde::Deserialize;

use crate::accident::index::cumulative_m;

/// 공사 지점이 경로에서 이 거리 안이면 경고한다(m)
pub const CONSTRUCTION_MATCH_M: f64 = 50.0;

/// 공사구간.csv 한 행
#[derive(Deserialize)]
struct Row {
    id: String,
    name: String,
    control: String,
    from_date: u32,
    to_date: u32,
    lat: f64,
    lon: f64,
}

/// 공사 하나
#[derive(Clone, Debug, PartialEq)]
pub struct Construction {
    pub id: String,
    pub name: String,
    /// 도로통제정보. 없으면 빈 문자열
    pub control: String,
    /// 시작일·종료일 YYYYMMDD
    pub from_date: u32,
    pub to_date: u32,
    pub lat: f64,
    pub lon: f64,
}

/// 경로가 지나는 공사 지점
#[derive(Clone, Debug, PartialEq)]
pub struct ConstructionHit {
    pub at_m: f64,
    pub construction: Construction,
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

/// 공사 지점 색인
pub struct ConstructionIndex {
    items: Vec<Construction>,
    tree: RTree<Located>,
}

/// 지금의 한국 시간 날짜 YYYYMMDD. 1970-01-01부터의 일수를 그레고리력 날짜로 바꾼다
pub fn today_kst() -> u32 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        + 9 * 3600;
    ymd_from_days((secs / 86_400) as i64)
}

/// 1970-01-01부터의 일수를 YYYYMMDD로 바꾼다(Howard Hinnant의 civil_from_days)
pub fn ymd_from_days(days: i64) -> u32 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y * 10_000 + m * 100 + d) as u32
}

impl ConstructionIndex {
    /// 공사구간.csv(id, police, road, name, control, from_date, to_date, lat, lon, address)를 읽는다
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut reader = csv::Reader::from_path(path)?;
        let mut items = Vec::new();
        for row in reader.deserialize::<Row>() {
            let row = row.map_err(io::Error::other)?;
            items.push(Construction {
                id: row.id,
                name: row.name,
                control: row.control,
                from_date: row.from_date,
                to_date: row.to_date,
                lat: row.lat,
                lon: row.lon,
            });
        }
        Ok(Self::build(items))
    }

    pub fn build(items: Vec<Construction>) -> Self {
        let tree = RTree::bulk_load(
            items
                .iter()
                .enumerate()
                .map(|(index, c)| Located {
                    index,
                    point: [c.lon, c.lat],
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

    /// 날짜 date(YYYYMMDD)에 진행 중이고 경로 좌표 열이 CONSTRUCTION_MATCH_M 안을 지나는 공사. 거리 순서이다
    pub fn hits(
        &self,
        geometry: &[(f64, f64)],
        distance_m: f64,
        date: u32,
    ) -> Vec<ConstructionHit> {
        let cumulative = cumulative_m(geometry, distance_m);
        let mut best: Vec<Option<(f64, f64)>> = vec![None; self.items.len()];
        let (pad_lat, pad_lon) = (
            CONSTRUCTION_MATCH_M / 110_950.0,
            CONSTRUCTION_MATCH_M / 88_000.0,
        );
        for (i, pair) in geometry.windows(2).enumerate() {
            let (p, q) = (pair[0], pair[1]);
            let envelope = AABB::from_corners(
                [p.1.min(q.1) - pad_lon, p.0.min(q.0) - pad_lat],
                [p.1.max(q.1) + pad_lon, p.0.max(q.0) + pad_lat],
            );
            for c in self.tree.locate_in_envelope_intersecting(envelope) {
                let item = &self.items[c.index];
                if date < item.from_date || date > item.to_date {
                    continue;
                }
                let cos = item.lat.to_radians().cos();
                let xy = |v: (f64, f64)| {
                    (
                        (v.1 - item.lon) * 111_320.0 * cos,
                        (v.0 - item.lat) * 110_950.0,
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
                if d > CONSTRUCTION_MATCH_M {
                    continue;
                }
                let at = cumulative[i] + t * (cumulative[i + 1] - cumulative[i]);
                if best[c.index].is_none_or(|(old, _)| d < old) {
                    best[c.index] = Some((d, at));
                }
            }
        }
        let mut out: Vec<ConstructionHit> = best
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                b.map(|(_, at_m)| ConstructionHit {
                    at_m,
                    construction: self.items[i].clone(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_hits() {
        assert_eq!(ymd_from_days(0), 19_700_101);
        assert_eq!(ymd_from_days(20_751), 20_261_025);
        let item = |id: &str, from_date, to_date, lat| Construction {
            id: id.to_string(),
            name: "하수관로 공사".to_string(),
            control: String::new(),
            from_date,
            to_date,
            lat,
            lon: 127.0045,
        };
        // 동쪽으로 가는 경로 북쪽 20m의 공사 두 건 중 하나는 2026-10-25에 끝났다
        let north = 37.5 + 20.0 / 110_950.0;
        let index = ConstructionIndex::build(vec![
            item("A", 20_260_101, 20_261_231, north),
            item("B", 20_250_101, 20_261_024, north),
        ]);
        let hits = index.hits(&[(37.5, 127.0), (37.5, 127.01)], 880.0, 20_261_025);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].construction.id, "A");
    }
}
