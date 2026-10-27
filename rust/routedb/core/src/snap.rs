use rstar::{AABB, PointDistance, RTree, RTreeObject};
use serde::Serialize;

use crate::file::GraphFile;

/// 위도 1도의 길이(m)
const M_PER_LAT: f64 = 111_320.0;
/// 경도 1도의 길이(m). 전국 중앙 위도 36도 기준이다.
const M_PER_LON: f64 = 111_320.0 * 0.809_017;

/// 거리가 같다고 보는 거리 제곱 차이(m²)
const TIE_D2: f64 = 1e-6;

/// 위경도를 m 단위 평면 좌표 (x, y)로 변환한다.
fn to_xy(lat: f64, lon: f64) -> [f64; 2] {
    [lon * M_PER_LON, lat * M_PER_LAT]
}

/// 평면 좌표를 위경도로 변환한다.
fn to_latlon(p: [f64; 2]) -> (f64, f64) {
    (p[1] / M_PER_LAT, p[0] / M_PER_LON)
}

/// 링크 선분 하나. 간선의 좌표 배열에서 그때그때 만든다
struct Segment {
    a: [f64; 2],
    b: [f64; 2],
    /// 간선 시작점에서 a까지의 거리(m)
    offset_a: f64,
}

impl Segment {
    /// 점 p를 선분 위에 투영한 위치와 선분 시작점으로부터의 비율 t(0~1)를 반환한다.
    fn project(&self, p: [f64; 2]) -> ([f64; 2], f64) {
        let (dx, dy) = (self.b[0] - self.a[0], self.b[1] - self.a[1]);
        let len2 = dx * dx + dy * dy;
        let t = if len2 == 0.0 {
            0.0
        } else {
            (((p[0] - self.a[0]) * dx + (p[1] - self.a[1]) * dy) / len2).clamp(0.0, 1.0)
        };
        ([self.a[0] + t * dx, self.a[1] + t * dy], t)
    }
}

/// R-tree에 넣는 간선 하나. 간선 좌표 전체를 감싸는 사각형이다
struct EdgeBox {
    edge: u32,
    envelope: AABB<[f64; 2]>,
}

impl RTreeObject for EdgeBox {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

impl PointDistance for EdgeBox {
    fn distance_2(&self, p: &[f64; 2]) -> f64 {
        self.envelope.distance_2(p)
    }
}

/// 간선 좌표 배열. 그래프 파일의 geom_first, geom_lat, geom_lon이다
#[derive(Clone, Copy)]
pub struct Geometry<'a> {
    pub first: &'a [u32],
    pub lat: &'a [f64],
    pub lon: &'a [f64],
}

impl<'a> From<&'a GraphFile> for Geometry<'a> {
    fn from(file: &'a GraphFile) -> Self {
        Self {
            first: file.geom_first(),
            lat: file.geom_lat(),
            lon: file.geom_lon(),
        }
    }
}

impl Geometry<'_> {
    /// 간선 e에서 점 p와 가장 가까운 선분 위치를 스냅으로 만든다. 거리 제곱과 함께 반환하며 선분이 없으면 None이다
    fn closest(&self, e: usize, p: [f64; 2]) -> Option<(f64, Snap)> {
        let (s, t) = (self.first[e] as usize, self.first[e + 1] as usize);
        let mut best: Option<(f64, Segment, [f64; 2], f64)> = None;
        let mut offset_a = 0.0;
        for i in s..t.saturating_sub(1) {
            let seg = Segment {
                a: to_xy(self.lat[i], self.lon[i]),
                b: to_xy(self.lat[i + 1], self.lon[i + 1]),
                offset_a,
            };
            let (q, ratio) = seg.project(p);
            let d2 = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
            offset_a += ((seg.b[0] - seg.a[0]).powi(2) + (seg.b[1] - seg.a[1]).powi(2)).sqrt();
            if best.as_ref().is_none_or(|b| d2 < b.0) {
                best = Some((d2, seg, q, ratio));
            }
        }
        let (d2, seg, q, ratio) = best?;
        let (lat, lon) = to_latlon(q);
        let (dx, dy) = (seg.b[0] - seg.a[0], seg.b[1] - seg.a[1]);
        let seg_len = (dx * dx + dy * dy).sqrt();
        Some((
            d2,
            Snap {
                edge: e as u32,
                distance_m: d2.sqrt(),
                lat,
                lon,
                offset_m: seg.offset_a + ratio * seg_len,
                bearing_deg: (dx.atan2(dy).to_degrees() + 360.0) % 360.0,
            },
        ))
    }
}

/// 스냅 결과
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Snap {
    /// 가장 가까운 간선 번호
    pub edge: u32,
    /// 입력 좌표에서 간선까지의 거리(m)
    pub distance_m: f64,
    /// 간선 위에 투영한 위치의 위도, 경도
    pub lat: f64,
    pub lon: f64,
    /// 간선 시작점에서 투영한 위치까지의 거리(m)
    pub offset_m: f64,
    /// 투영한 선분의 진행 방위각(도). 진북이 0이고 시계 방향이다
    pub bearing_deg: f64,
}

/// 간선 R-tree. 간선마다 좌표를 감싸는 사각형 하나를 넣고, 가까운 간선을 찾은 뒤 그 간선의 선분만 계산한다
pub struct LinkIndex {
    tree: RTree<EdgeBox>,
}

impl LinkIndex {
    /// 선분이 있는 모든 간선의 좌표 사각형으로 R-tree를 만든다.
    pub fn build(geom_first: &[u32], geom_lat: &[f64], geom_lon: &[f64]) -> Self {
        let mut boxes = Vec::with_capacity(geom_first.len().saturating_sub(1));
        for e in 0..geom_first.len() - 1 {
            let (s, t) = (geom_first[e] as usize, geom_first[e + 1] as usize);
            if t < s + 2 {
                continue;
            }
            let points: Vec<[f64; 2]> = (s..t).map(|i| to_xy(geom_lat[i], geom_lon[i])).collect();
            boxes.push(EdgeBox {
                edge: e as u32,
                envelope: AABB::from_points(points.iter()),
            });
        }
        Self {
            tree: RTree::bulk_load(boxes),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }

    /// R-tree에 넣은 간선 수
    pub fn len(&self) -> usize {
        self.tree.size()
    }

    /// 위경도에서 가장 가까운 간선을 찾는다.
    pub fn nearest(&self, geom: Geometry<'_>, lat: f64, lon: f64) -> Option<Snap> {
        self.nearest_n(geom, lat, lon, 1).into_iter().next()
    }

    /// 위경도에서 가까운 순서로 서로 다른 간선 n개를 찾는다. n번째와 거리가 같은 간선(양방향 도로의 반대 방향 등)은 모두 넣는다.
    /// 사각형까지의 거리가 가까운 간선부터 실제 거리를 계산하고, 사각형 거리가 n번째 실제 거리보다 멀어지면 멈춘다
    pub fn nearest_n(&self, geom: Geometry<'_>, lat: f64, lon: f64, n: usize) -> Vec<Snap> {
        let p = to_xy(lat, lon);
        let mut found: Vec<(f64, Snap)> = Vec::with_capacity(n + 4);
        for (b, box_d2) in self.tree.nearest_neighbor_iter_with_distance_2(p) {
            if found.len() >= n && box_d2 > found[n - 1].0 + TIE_D2 {
                break;
            }
            if let Some((d2, snap)) = geom.closest(b.edge as usize, p) {
                let at = found.partition_point(|(x, _)| *x <= d2);
                found.insert(at, (d2, snap));
            }
        }
        if let Some(&(kth, _)) = found.get(n.saturating_sub(1)) {
            found.retain(|(d2, _)| *d2 <= kth + TIE_D2);
        }
        found.into_iter().map(|(_, snap)| snap).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_segment() {
        // 간선 0은 위도 37.000 위에서 동서로, 간선 1은 경도 127.002 위에서 남북으로 놓인 선분이다
        let geom_first = [0, 2, 4];
        let geom_lat = [37.000, 37.000, 37.000, 37.002];
        let geom_lon = [127.000, 127.002, 127.002, 127.002];
        let index = LinkIndex::build(&geom_first, &geom_lat, &geom_lon);
        assert_eq!(index.len(), 2);
        let geom = Geometry {
            first: &geom_first,
            lat: &geom_lat,
            lon: &geom_lon,
        };

        // 간선 0 중간에서 북쪽으로 약 11m 떨어진 점
        let s = index.nearest(geom, 37.0001, 127.001).unwrap();
        assert_eq!(s.edge, 0);
        assert!((s.distance_m - 11.1).abs() < 0.5, "{}", s.distance_m);
        assert!((s.lat - 37.000).abs() < 1e-9 && (s.lon - 127.001).abs() < 1e-9);
        // 간선 0은 경도 0.002도 길이 약 180m이고 투영 위치는 그 중간이다
        assert!((s.offset_m - 90.06).abs() < 0.1, "{}", s.offset_m);
        // 동쪽으로 가는 선분의 방위각은 90도이다
        assert!((s.bearing_deg - 90.0).abs() < 1e-9, "{}", s.bearing_deg);

        // 간선 1 옆. 북쪽으로 가는 선분의 방위각은 0도이다
        let s = index.nearest(geom, 37.0015, 127.0021).unwrap();
        assert_eq!(s.edge, 1);
        assert!(s.bearing_deg.abs() < 1e-9, "{}", s.bearing_deg);
    }
}
