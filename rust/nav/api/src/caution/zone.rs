//! 보호구역(어린이, 노인, 장애인). scripts/caution/build_zones.py가 만든 보호구역.csv를 읽는다.
//! 위경도는 보호구역 중심이며 표준데이터 정의가 "주변 300m를 보호구역으로 지정하기 위해 선정한 시설"이라
//! 경로가 중심에서 ZONE_RADIUS_M 안을 지나는 부분을 보호구역 구간으로 본다. 같은 종류의 구간이 겹치거나 이어지면 합친다

use std::io;
use std::path::Path;

use rstar::{AABB, RTree, RTreeObject};
use serde::{Deserialize, Serialize};

use crate::accident::index::cumulative_m;

/// 보호구역 반경(m)
pub const ZONE_RADIUS_M: f64 = 300.0;

/// 보호구역 종류
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ZoneKind {
    /// 어린이보호구역
    Child,
    /// 노인보호구역
    Elderly,
    /// 장애인보호구역
    Disabled,
}

/// 보호구역.csv 한 행
#[derive(Deserialize)]
struct Row {
    kind: ZoneKind,
    name: String,
    lat: f64,
    lon: f64,
    limit: String,
}

/// 보호구역 하나
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProtectZone {
    pub kind: ZoneKind,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// 제한속도(km/h). 노인·장애인보호구역 표준데이터에만 있다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u8>,
}

/// 경로가 지나는 같은 종류의 보호구역 구간
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ZoneSection {
    pub kind: ZoneKind,
    pub from_m: f64,
    pub to_m: f64,
    pub zones: Vec<ProtectZone>,
}

impl ZoneSection {
    /// 구간 보호구역들의 제한속도 중 가장 낮은 값
    pub fn limit(&self) -> Option<u8> {
        self.zones.iter().filter_map(|z| z.limit).min()
    }
}

struct Center {
    index: usize,
    point: [f64; 2],
}

impl RTreeObject for Center {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.point)
    }
}

/// 보호구역 중심 색인. 좌표는 [경도, 위도]
pub struct ZoneIndex {
    zones: Vec<ProtectZone>,
    tree: RTree<Center>,
}

impl ZoneIndex {
    /// 보호구역.csv(kind, name, lat, lon, limit)를 읽는다
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut reader = csv::Reader::from_path(path)?;
        let mut zones = Vec::new();
        for row in reader.deserialize::<Row>() {
            let row = row.map_err(io::Error::other)?;
            zones.push(ProtectZone {
                kind: row.kind,
                name: row.name,
                lat: row.lat,
                lon: row.lon,
                limit: row.limit.trim().parse().ok(),
            });
        }
        Ok(Self::build(zones))
    }

    pub fn build(zones: Vec<ProtectZone>) -> Self {
        let centers = zones
            .iter()
            .enumerate()
            .map(|(index, z)| Center {
                index,
                point: [z.lon, z.lat],
            })
            .collect();
        Self {
            zones,
            tree: RTree::bulk_load(centers),
        }
    }

    pub fn len(&self) -> usize {
        self.zones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.zones.is_empty()
    }

    /// 경로 좌표 열 (위도, 경도)이 지나는 보호구역 구간. 종류별로 겹치거나 이어지는 구간을 합치고 시작 거리 순서이다
    pub fn sections(&self, geometry: &[(f64, f64)], distance_m: f64) -> Vec<ZoneSection> {
        let cumulative = cumulative_m(geometry, distance_m);
        let mut spans: Vec<Option<(f64, f64)>> = vec![None; self.zones.len()];
        let (pad_lat, pad_lon) = (ZONE_RADIUS_M / 110_950.0, ZONE_RADIUS_M / 88_000.0);
        for (i, pair) in geometry.windows(2).enumerate() {
            let (p, q) = (pair[0], pair[1]);
            let envelope = AABB::from_corners(
                [p.1.min(q.1) - pad_lon, p.0.min(q.0) - pad_lat],
                [p.1.max(q.1) + pad_lon, p.0.max(q.0) + pad_lat],
            );
            let (from, to) = (cumulative[i], cumulative[i + 1]);
            for c in self.tree.locate_in_envelope_intersecting(envelope) {
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
                let len = dx.hypot(dy);
                // 선분과 원이 만나는 구간 [t0, t1] (선분 길이 비율)
                let (t0, t1) = if len == 0.0 {
                    if a.0.hypot(a.1) > ZONE_RADIUS_M {
                        continue;
                    }
                    (0.0, 1.0)
                } else {
                    let t = -(a.0 * dx + a.1 * dy) / (len * len);
                    let d = (a.0 + t * dx).hypot(a.1 + t * dy);
                    if d > ZONE_RADIUS_M {
                        continue;
                    }
                    let half = (ZONE_RADIUS_M * ZONE_RADIUS_M - d * d).sqrt() / len;
                    ((t - half).max(0.0), (t + half).min(1.0))
                };
                if t0 > t1 {
                    continue;
                }
                let (s, e) = (from + t0 * (to - from), from + t1 * (to - from));
                let span = &mut spans[c.index];
                *span = Some(match *span {
                    Some((first, last)) => (first.min(s), last.max(e)),
                    None => (s, e),
                });
            }
        }
        let mut hits: Vec<(usize, f64, f64)> = spans
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|(a, b)| (i, a, b)))
            .collect();
        hits.sort_by(|a, b| {
            self.zones[a.0]
                .kind
                .cmp(&self.zones[b.0].kind)
                .then(a.1.total_cmp(&b.1))
        });
        let mut out: Vec<ZoneSection> = Vec::new();
        for (i, from_m, to_m) in hits {
            let zone = &self.zones[i];
            if let Some(last) = out.last_mut()
                && last.kind == zone.kind
                && from_m <= last.to_m
            {
                last.to_m = last.to_m.max(to_m);
                last.zones.push(zone.clone());
                continue;
            }
            out.push(ZoneSection {
                kind: zone.kind,
                from_m,
                to_m,
                zones: vec![zone.clone()],
            });
        }
        out.sort_by(|a, b| a.from_m.total_cmp(&b.from_m));
        out
    }
}

/// 보호구역 구간에서 excluded 구간(고속국도·도시고속국도 등)과 겹치는 부분을 뺀다. 남는 조각이 1m 미만이면 버린다
pub fn excluding(sections: Vec<ZoneSection>, excluded: &[(f64, f64)]) -> Vec<ZoneSection> {
    let mut out = Vec::new();
    for s in sections {
        let mut pieces = vec![(s.from_m, s.to_m)];
        for &(a, b) in excluded {
            pieces = pieces
                .into_iter()
                .flat_map(|(x, y)| {
                    if b <= x || a >= y {
                        vec![(x, y)]
                    } else {
                        let mut keep = Vec::new();
                        if a > x {
                            keep.push((x, a));
                        }
                        if b < y {
                            keep.push((b, y));
                        }
                        keep
                    }
                })
                .collect();
        }
        for (x, y) in pieces {
            if y - x >= 1.0 {
                out.push(ZoneSection {
                    kind: s.kind,
                    from_m: x,
                    to_m: y,
                    zones: s.zones.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(kind: ZoneKind, name: &str, lat: f64, lon: f64, limit: Option<u8>) -> ProtectZone {
        ProtectZone {
            kind,
            name: name.to_string(),
            lat,
            lon,
            limit,
        }
    }

    #[test]
    fn route_through_zones() {
        // 동쪽으로 2km 가는 경로. 초등학교 두 곳(500m, 700m, 경로 위)은 한 구간으로 합치고, 노인보호구역(1,500m, 북쪽 200m)은
        // 원과 만나는 길이가 2 × √(300² − 200²) ≈ 447m이며, 북쪽 400m 떨어진 유치원은 경로를 지나지 않는다
        let east = |m: f64| 127.0 + m / (111_320.0 * 37.5f64.to_radians().cos());
        let north = |m: f64| 37.5 + m / 110_950.0;
        let index = ZoneIndex::build(vec![
            zone(ZoneKind::Child, "가초등학교", 37.5, east(500.0), None),
            zone(ZoneKind::Child, "나초등학교", 37.5, east(700.0), None),
            zone(
                ZoneKind::Elderly,
                "다경로당",
                north(200.0),
                east(1_500.0),
                Some(30),
            ),
            zone(
                ZoneKind::Child,
                "라유치원",
                north(400.0),
                east(1_000.0),
                None,
            ),
        ]);
        let geometry = vec![(37.5, 127.0), (37.5, east(2_000.0))];
        let s = index.sections(&geometry, 2_000.0);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].kind, ZoneKind::Child);
        assert_eq!(s[0].zones.len(), 2);
        assert!(
            (s[0].from_m - 200.0).abs() < 2.0 && (s[0].to_m - 1_000.0).abs() < 2.0,
            "{s:?}"
        );
        assert_eq!(s[1].kind, ZoneKind::Elderly);
        assert!(((s[1].to_m - s[1].from_m) - 447.2).abs() < 2.0, "{s:?}");
        assert_eq!(s[1].limit(), Some(30));
        // 900~1,400m가 고속국도이면 어린이보호구역 200~1,000m는 200~900m만 남고 노인보호구역은 1,400m 뒤만 남는다
        let kept = excluding(s, &[(900.0, 1_400.0)]);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].to_m - 900.0).abs() < 1e-9);
        assert!((kept[1].from_m - 1_400.0).abs() < 1e-9);
    }
}
