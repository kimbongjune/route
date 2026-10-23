use rstar::{AABB, PointDistance, RTree, RTreeObject};

/// 위도 1도의 길이(m)
const M_PER_LAT: f64 = 111_320.0;
/// 경도 1도의 길이(m). 전국 중앙 위도 36도 기준이다.
const M_PER_LON: f64 = 111_320.0 * 0.809_017;

/// 위경도를 m 단위 평면 좌표 (x, y)로 변환한다.
fn to_xy(lat: f64, lon: f64) -> [f64; 2] {
    [lon * M_PER_LON, lat * M_PER_LAT]
}

/// 평면 좌표를 위경도로 변환한다.
fn to_latlon(p: [f64; 2]) -> (f64, f64) {
    (p[1] / M_PER_LAT, p[0] / M_PER_LON)
}

/// R-tree에 넣는 링크 선분 하나
pub struct Segment {
    pub edge: u32,
    pub a: [f64; 2],
    pub b: [f64; 2],
    /// 간선 시작점에서 a까지의 거리(m)
    pub offset_a: f64,
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

impl RTreeObject for Segment {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(self.a, self.b)
    }
}

impl PointDistance for Segment {
    fn distance_2(&self, p: &[f64; 2]) -> f64 {
        let (q, _) = self.project(*p);
        (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)
    }
}

/// 스냅 결과
#[derive(Clone, Debug, PartialEq)]
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
}

/// 링크 선분 R-tree
pub struct LinkIndex {
    tree: RTree<Segment>,
}

impl LinkIndex {
    /// 모든 간선의 좌표를 선분으로 나누어 R-tree를 만든다.
    pub fn build(geom_first: &[u32], geom_lat: &[f64], geom_lon: &[f64]) -> Self {
        let mut segments = Vec::with_capacity(geom_lat.len());
        for e in 0..geom_first.len() - 1 {
            let (s, t) = (geom_first[e] as usize, geom_first[e + 1] as usize);
            let mut offset_a = 0.0;
            for i in s..t.saturating_sub(1) {
                let (a, b) = (
                    to_xy(geom_lat[i], geom_lon[i]),
                    to_xy(geom_lat[i + 1], geom_lon[i + 1]),
                );
                segments.push(Segment {
                    edge: e as u32,
                    a,
                    b,
                    offset_a,
                });
                offset_a += ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            }
        }
        Self {
            tree: RTree::bulk_load(segments),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }

    pub fn len(&self) -> usize {
        self.tree.size()
    }

    /// 위경도에서 가장 가까운 간선을 찾는다.
    pub fn nearest(&self, lat: f64, lon: f64) -> Option<Snap> {
        self.nearest_n(lat, lon, 1).pop()
    }

    /// 위경도에서 가까운 순서로 서로 다른 간선 n개를 찾는다.
    pub fn nearest_n(&self, lat: f64, lon: f64, n: usize) -> Vec<Snap> {
        let p = to_xy(lat, lon);
        let mut snaps: Vec<Snap> = Vec::with_capacity(n);
        for seg in self.tree.nearest_neighbor_iter(p) {
            if snaps.iter().any(|s| s.edge == seg.edge) {
                continue;
            }
            let (q, t) = seg.project(p);
            let (lat, lon) = to_latlon(q);
            let seg_len = ((seg.b[0] - seg.a[0]).powi(2) + (seg.b[1] - seg.a[1]).powi(2)).sqrt();
            snaps.push(Snap {
                edge: seg.edge,
                distance_m: seg.distance_2(&p).sqrt(),
                lat,
                lon,
                offset_m: seg.offset_a + t * seg_len,
            });
            if snaps.len() == n {
                break;
            }
        }
        snaps
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

        // 간선 0 중간에서 북쪽으로 약 11m 떨어진 점
        let s = index.nearest(37.0001, 127.001).unwrap();
        assert_eq!(s.edge, 0);
        assert!((s.distance_m - 11.1).abs() < 0.5, "{}", s.distance_m);
        assert!((s.lat - 37.000).abs() < 1e-9 && (s.lon - 127.001).abs() < 1e-9);
        // 간선 0은 경도 0.002도 길이 약 180m이고 투영 위치는 그 중간이다
        assert!((s.offset_m - 90.06).abs() < 0.1, "{}", s.offset_m);

        // 간선 1 옆
        let s = index.nearest(37.0015, 127.0021).unwrap();
        assert_eq!(s.edge, 1);
    }
}
