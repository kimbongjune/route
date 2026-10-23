use std::io;
use std::path::Path;

use cch::bundle::{CchBundle, CchView, MetricBundle, MetricView};
use cch::path::PathQuery;

use crate::file::GraphFile;
use crate::hierarchy;
use crate::profile;
use crate::snap::{LinkIndex, Snap};

/// 가중치 프로파일
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Profile {
    /// 거리
    Distance,
    /// 최소시간
    Time,
    /// 무료도로
    Free,
}

/// 경로 위 지점의 종류
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PoiKind {
    /// 무인교통단속카메라
    Camera,
    /// 과속방지턱
    Bump,
}

/// 경로 위 지점. at_m은 출발점부터의 거리이다.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutePoi {
    pub kind: PoiKind,
    /// 카메라번호 또는 방지턱 지점번호
    pub id: u32,
    pub at_m: f64,
    pub lat: f64,
    pub lon: f64,
    /// 카메라 제한속도(km/h). 0은 없음
    pub limit: u8,
    /// 카메라 단속구분 비트(1 속도, 2 신호, 4 통행위반, 8 불법주정차, 16 기타). 방지턱은 1이 연속형이다
    pub enforce: u8,
}

/// 경로 위 구간. 출발점부터의 시작·끝 거리이다.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub from_m: f64,
    pub to_m: f64,
}

/// 같은 도로명이 이어지는 구간
#[derive(Clone, Debug, PartialEq)]
pub struct RoadSegment {
    /// 도로명. 없으면 빈 문자열
    pub name: String,
    pub from_m: f64,
    pub to_m: f64,
    /// 구간의 링크 수
    pub links: usize,
}

/// 간선 열의 도로명 번호와 지나는 길이를 받아 같은 도로명이 이어지는 구간으로 합친다.
/// 반환값은 (도로명 번호, 시작 거리, 끝 거리, 링크 수)이다.
pub fn merge_runs(names: &[u16], lengths: &[f64]) -> Vec<(u16, f64, f64, usize)> {
    let mut runs: Vec<(u16, f64, f64, usize)> = Vec::new();
    let mut walked = 0.0;
    for (&name, &len) in names.iter().zip(lengths) {
        match runs.last_mut() {
            Some(run) if run.0 == name => {
                run.2 += len;
                run.3 += 1;
            }
            _ => runs.push((name, walked, walked + len, 1)),
        }
        walked += len;
    }
    runs
}

/// 경로
#[derive(Clone, Debug)]
pub struct Route {
    pub profile: Profile,
    pub from: Snap,
    pub to: Snap,
    /// 출발 스냅 지점부터 도착 스냅 지점까지의 거리(m)
    pub distance_m: f64,
    /// 링크 길이를 제한속도로 나눈 시간의 합(초)
    pub duration_s: f64,
    /// 지나는 LINK_ID 열
    pub links: Vec<u64>,
    /// 경로 좌표 (위도, 경도)
    pub geometry: Vec<(f64, f64)>,
    /// 지나는 요금소의 영업소코드. 바로 앞 링크에도 있던 영업소코드는 같은 요금소이므로 한 번만 넣는다
    pub tolls: Vec<u32>,
    /// 경로 위 카메라와 방지턱. 불법주정차만 단속하는 카메라는 뺀다
    pub pois: Vec<RoutePoi>,
    /// 같은 도로명이 이어지는 구간
    pub segments: Vec<RoadSegment>,
    /// 스쿨존 구간. 스쿨존 데이터를 적재하기 전까지 비어 있다
    pub school_zones: Vec<Section>,
    /// 경로 링크의 통과제한높이 최솟값(cm). 0은 제한 없음
    pub min_height_cm: u16,
    /// 경로 링크의 통과제한하중 최솟값(kg). 0은 제한 없음
    pub min_weight_kg: u16,
    /// 경로 링크에 있는 통행제한차량 코드
    pub rest_veh: Vec<u8>,
}

/// 그래프 파일, 링크 R-tree, CCH 구조, 프로파일별 metric
pub struct RouteDb {
    pub file: GraphFile,
    pub index: LinkIndex,
    cch: CchBundle,
    metrics: [MetricBundle; 3],
}

/// 경로 탐색기. CCH 쿼리 버퍼를 갖고 있어 여러 경로에 재사용한다.
pub struct Router<'a> {
    db: &'a RouteDb,
    metrics: [MetricView<'a>; 3],
    query: PathQuery<'a>,
}

/// 좌표 목록의 평면 길이(m)
fn planar_length(points: &[(f64, f64)]) -> f64 {
    points.windows(2).map(|w| planar_distance(w[0], w[1])).sum()
}

fn planar_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dy = (b.0 - a.0) * 111_320.0;
    let dx = (b.1 - a.1) * 111_320.0 * 0.809_017;
    (dx * dx + dy * dy).sqrt()
}

/// 좌표 목록에서 길이 비율 from..to 부분을 잘라낸다. 양 끝은 선분 위를 보간한 점이다.
pub fn sub_polyline(points: &[(f64, f64)], from: f64, to: f64) -> Vec<(f64, f64)> {
    let total = planar_length(points);
    if points.len() < 2 || total == 0.0 {
        return points.to_vec();
    }
    let (start, end) = (from.clamp(0.0, 1.0) * total, to.clamp(0.0, 1.0) * total);
    let mut out = vec![point_at(points, start)];
    let mut walked = 0.0;
    for w in points.windows(2) {
        let seg = planar_distance(w[0], w[1]);
        if walked > start && walked < end {
            out.push(w[0]);
        }
        walked += seg;
    }
    out.push(point_at(points, end));
    out
}

/// 좌표 목록에서 시작점부터 at(m) 떨어진 점
pub fn point_at(points: &[(f64, f64)], at: f64) -> (f64, f64) {
    let mut walked = 0.0;
    for w in points.windows(2) {
        let seg = planar_distance(w[0], w[1]);
        if walked + seg >= at && seg > 0.0 {
            let t = ((at - walked) / seg).clamp(0.0, 1.0);
            return (
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
            );
        }
        walked += seg;
    }
    *points.last().unwrap()
}

impl RouteDb {
    /// 그래프 파일, CCH 구조, 거리·최소시간·무료도로 metric 순서의 경로를 받아 연다.
    pub fn open(graph: &Path, cch_struct: &Path, metrics: [&Path; 3]) -> io::Result<Self> {
        let file = GraphFile::open(graph)?;
        let index = LinkIndex::build(file.geom_first(), file.geom_lat(), file.geom_lon());
        let cch = CchBundle::open(cch_struct)?;
        let metrics = [
            MetricBundle::open(metrics[0])?,
            MetricBundle::open(metrics[1])?,
            MetricBundle::open(metrics[2])?,
        ];
        Ok(Self {
            file,
            index,
            cch,
            metrics,
        })
    }

    /// 간선 e의 좌표 (위도, 경도) 목록
    pub fn edge_points(&self, e: usize) -> Vec<(f64, f64)> {
        let (s, t) = (
            self.file.geom_first()[e] as usize,
            self.file.geom_first()[e + 1] as usize,
        );
        (s..t)
            .map(|i| (self.file.geom_lat()[i], self.file.geom_lon()[i]))
            .collect()
    }

    /// 스냅 위치가 간선 시작점에서 얼마나 떨어졌는지를 간선 길이 기준 비율(0~1)로 계산한다.
    pub fn fraction(&self, snap: &Snap) -> f64 {
        let planar = planar_length(&self.edge_points(snap.edge as usize));
        if planar == 0.0 {
            0.0
        } else {
            (snap.offset_m / planar).clamp(0.0, 1.0)
        }
    }

    /// CCH 구조의 view. `router`에 넘긴다.
    pub fn cch_view(&self) -> CchView<'_> {
        self.cch.view()
    }

    /// 경로 탐색기를 만든다. 쿼리 버퍼를 교차로 수만큼 할당하므로 한 번 만들어 재사용한다.
    pub fn router<'a>(&'a self, cch_view: &'a CchView<'a>) -> Router<'a> {
        Router {
            db: self,
            metrics: [
                self.metrics[0].view(),
                self.metrics[1].view(),
                self.metrics[2].view(),
            ],
            query: PathQuery::new(cch_view),
        }
    }
}

impl Router<'_> {
    /// 출발·도착 위경도로 경로를 구한다. 가까운 간선 5개씩의 25개 조합 중 출발 스냅 거리, 경로 거리, 도착 스냅 거리의 합이 가장 작은 조합이다.
    pub fn route(&mut self, from: (f64, f64), to: (f64, f64), profile: Profile) -> Option<Route> {
        let db = self.db;
        let file = &db.file;
        let (turn_first_out, turn_head, turn_weight, length) = (
            file.turn_first_out(),
            file.turn_head(),
            file.turn_weight(),
            file.length(),
        );
        let metric_view = &self.metrics[profile as usize];
        let query = &mut self.query;
        let froms = db.index.nearest_n(from.0, from.1, 5);
        let tos = db.index.nearest_n(to.0, to.1, 5);
        let mut best: Option<(Snap, Snap, f64, Vec<u32>)> = None;
        for from in &froms {
            for to in &tos {
                let (from_frac, to_frac) = (db.fraction(from), db.fraction(to));
                let found = if from.edge == to.edge && from_frac <= to_frac {
                    Some((
                        length[from.edge as usize] * (to_frac - from_frac),
                        vec![from.edge],
                    ))
                } else {
                    query.path(metric_view, from.edge, to.edge).map(|nodes| {
                        let turns = hierarchy::edges_of_node_path(
                            turn_first_out,
                            turn_head,
                            turn_weight,
                            &nodes,
                        );
                        let mut edges = vec![from.edge];
                        for &t in &turns {
                            edges.extend_from_slice(file.via_of(t));
                            edges.push(turn_head[t]);
                        }
                        let between: f64 = edges[1..edges.len() - 1]
                            .iter()
                            .map(|&e| length[e as usize])
                            .sum();
                        (
                            length[from.edge as usize] * (1.0 - from_frac)
                                + between
                                + length[to.edge as usize] * to_frac,
                            edges,
                        )
                    })
                };
                if let Some((total, edges)) = found {
                    let cost = from.distance_m + total + to.distance_m;
                    if best
                        .as_ref()
                        .is_none_or(|b| cost < b.0.distance_m + b.2 + b.1.distance_m)
                    {
                        best = Some((from.clone(), to.clone(), total, edges));
                    }
                }
            }
        }
        let (from, to, distance_m, edges) = best?;
        Some(db.build(profile, from, to, distance_m, &edges))
    }
}

impl RouteDb {
    /// 간선 열로 Route를 만든다. 출발·도착 간선은 스냅 위치 비율만큼만 포함한다.
    pub fn build(
        &self,
        profile: Profile,
        from: Snap,
        to: Snap,
        distance_m: f64,
        edges: &[u32],
    ) -> Route {
        let file = &self.file;
        let (length, max_spd) = (file.length(), file.max_spd());
        let (from_frac, to_frac) = (self.fraction(&from), self.fraction(&to));
        let single = edges.len() == 1;
        let mut geometry = Vec::new();
        let mut pois = Vec::new();
        let mut tolls: Vec<u32> = Vec::new();
        let mut prev: &[u32] = &[];
        let mut duration_ms = 0.0;
        let mut walked = 0.0;
        let (mut min_h, mut min_w, mut veh): (u16, u16, Vec<u8>) = (0, 0, Vec::new());
        let mut names = Vec::with_capacity(edges.len());
        let mut parts = Vec::with_capacity(edges.len());
        for (i, &e) in edges.iter().enumerate() {
            let e = e as usize;
            // 이 간선에서 지나는 길이 비율
            let lo = if i == 0 { from_frac } else { 0.0 };
            let hi = if i == edges.len() - 1 { to_frac } else { 1.0 };
            let part = if single { to_frac - from_frac } else { hi - lo };
            let points = self.edge_points(e);
            let piece = sub_polyline(&points, lo, hi);
            geometry.extend(piece.iter().skip(if geometry.is_empty() { 0 } else { 1 }));
            duration_ms += profile::link_time_ms(length, max_spd, e) * part;
            // 요금소
            let cur = file.tolls_of(e);
            tolls.extend(cur.iter().filter(|u| !prev.contains(u)));
            prev = cur;
            // 통과제한
            let (h, w, v) = (file.rest_h()[e], file.rest_w()[e], file.rest_veh()[e]);
            if h > 0 && (min_h == 0 || h < min_h) {
                min_h = h;
            }
            if w > 0 && (min_w == 0 || w < min_w) {
                min_w = w;
            }
            if v > 0 && !veh.contains(&v) {
                veh.push(v);
            }
            // 지점. 간선 안 위치 비율이 지나는 범위 안인 것만 넣고 거리는 출발점부터 잰다
            let planar = planar_length(&points);
            for p in file.poi_range(e) {
                let at = file.poi_at()[p] as f64 / 10_000.0;
                if at < lo || at > hi || file.poi_enforce()[p] == 8 && file.poi_kind()[p] == 1 {
                    continue;
                }
                let (lat, lon) = point_at(&points, at * planar);
                pois.push(RoutePoi {
                    kind: if file.poi_kind()[p] == 1 {
                        PoiKind::Camera
                    } else {
                        PoiKind::Bump
                    },
                    id: file.poi_id()[p],
                    at_m: walked + (at - lo) * length[e],
                    lat,
                    lon,
                    limit: file.poi_limit()[p],
                    enforce: file.poi_enforce()[p],
                });
            }
            walked += part * length[e];
            names.push(file.road_name()[e]);
            parts.push(part * length[e]);
        }
        let segments = merge_runs(&names, &parts)
            .into_iter()
            .map(|(name, from_m, to_m, links)| RoadSegment {
                name: file.name(name as usize).to_string(),
                from_m,
                to_m,
                links,
            })
            .collect();
        veh.sort_unstable();
        Route {
            profile,
            from,
            to,
            distance_m,
            duration_s: duration_ms / 1000.0,
            links: edges.iter().map(|&e| file.link_id()[e as usize]).collect(),
            geometry,
            tolls,
            pois,
            segments,
            school_zones: Vec::new(),
            min_height_cm: min_h,
            min_weight_kg: min_w,
            rest_veh: veh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_same_names() {
        // 도로명 1, 1, 2, 1, 1 순서의 링크 5개는 세 구간이 된다
        let runs = merge_runs(&[1, 1, 2, 1, 1], &[10.0, 20.0, 5.0, 1.0, 2.0]);
        assert_eq!(
            runs,
            [(1, 0.0, 30.0, 2), (2, 30.0, 35.0, 1), (1, 35.0, 38.0, 2)]
        );
        assert_eq!(merge_runs(&[], &[]), []);
    }

    #[test]
    fn sub_polyline_and_point_at() {
        // 동쪽으로 100m, 북쪽으로 100m 가는 꺾인 선. 평면 길이는 각각 약 90m, 111m이다
        let pts = [
            (37.0, 127.0),
            (37.0, 127.0 + 100.0 / (111_320.0 * 0.809_017)),
            (
                37.0 + 100.0 / 111_320.0,
                127.0 + 100.0 / (111_320.0 * 0.809_017),
            ),
        ];
        let total = planar_length(&pts);
        assert!((total - 200.0).abs() < 0.01);
        // 앞 25%와 뒤 25%를 잘라내면 모퉁이가 남고 양 끝은 선분 위의 점이다
        let part = sub_polyline(&pts, 0.25, 0.75);
        assert_eq!(part.len(), 3);
        assert_eq!(part[1], pts[1]);
        assert!((planar_length(&part) - 100.0).abs() < 0.01);
        assert!((part[0].0 - 37.0).abs() < 1e-9 && part[0].1 > 127.0);
        assert!((part[2].1 - pts[2].1).abs() < 1e-9 && part[2].0 < pts[2].0);
        // 150m 지점은 두 번째 선분의 가운데
        let p = point_at(&pts, 150.0);
        assert!((p.0 - (37.0 + 50.0 / 111_320.0)).abs() < 1e-9);
        assert_eq!(point_at(&pts, 500.0), pts[2]);
    }
}
