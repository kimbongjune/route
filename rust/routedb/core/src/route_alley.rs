//! 골목을 지나는 경로. 출발·도착이 큰길보다 골목에 가까우면 골목망을 탐색하여 큰길 진입점(포털)을 출발·도착 후보로 하고,
//! 고른 큰길 경로의 앞뒤에 골목 구간을 잇는다. 출발과 도착이 같은 골목망 안이면 골목만 지나는 경로와도 비교한다

use std::io;
use std::path::Path;

use crate::alley::{AlleyFile, AlleySnap, Piece, Search};
use crate::route::{
    Profile, RoadSegment, Route, RouteDb, Step, Turn, classify, end_bearings, heading_diff,
    planar_length, point_at, sign_color, turn_angle,
};
use crate::snap::Snap;

/// 골목 스냅을 찾는 최대 거리(m)
pub const ALLEY_SNAP_MAX_M: f64 = 150.0;
/// 골목 스냅이 가장 가까운 큰길 스냅보다 이만큼 이상 가까워야 골목에서 출발·도착한다(m)
pub const ALLEY_SNAP_MARGIN_M: f64 = 5.0;
/// 골목망 탐색의 최대 거리(m)
pub const ALLEY_SEARCH_M: f64 = 2_000.0;
/// 골목망 탐색에서 모으는 포털 노드 수
pub const ALLEY_MAX_PORTALS: usize = 8;
/// 큰길 경로 탐색에 넣는 골목 쪽 출발·도착 후보 수
pub const ALLEY_MAX_CANDIDATES: usize = 5;
/// 골목 소요시간에 쓰는 속도(km/h). 표준노드링크가 마을안길에 준 MAX_SPD 10과 같다
pub const ALLEY_SPEED_KMH: f64 = 10.0;
/// 최소시간·무료도로에서 후보를 고를 때 골목 길이에 곱하는 배율. 큰길보다 느린 골목을 덜 지나게 한다
pub const ALLEY_TIME_FACTOR: f64 = 3.0;

/// 후보를 고르는 비용에서 골목 길이에 곱하는 배율. 최단거리는 1이다
pub fn alley_factor(profile: Profile) -> f64 {
    if profile == Profile::Distance {
        1.0
    } else {
        ALLEY_TIME_FACTOR
    }
}

/// 골목 쪽 출발 또는 도착. 스냅, 골목망 탐색 결과, 포털마다 (큰길 스냅, 스냅 지점에서 포털까지 조각 열)
pub struct AlleyEnd {
    pub snap: AlleySnap,
    pub search: Search,
    pub candidates: Vec<(Snap, Vec<Piece>)>,
}

/// 조각 열을 반대 방향으로 바꾼다
pub fn reverse_pieces(pieces: &[Piece]) -> Vec<Piece> {
    pieces
        .iter()
        .rev()
        .map(|p| Piece {
            link: p.link,
            from_frac: p.to_frac,
            to_frac: p.from_frac,
        })
        .collect()
}

/// 골목 링크의 안내 표지 색을 정하는 도로등급. 108 기타
const ALLEY_RANK: u8 = 108;

/// 골목과 큰길이 바뀌는 자리의 안내 지점. 앞 구간 끝 15m와 뒤 구간 처음 15m의 방위각 차이로 회전을 정하며,
/// 직진이거나 도로명이 같은 50° 미만의 꺾임이면 None이다. 분기 수는 큰길 양쪽이므로 2로 한다
pub(crate) fn boundary_step(
    at_m: f64,
    before: &[(f64, f64)],
    after: &[(f64, f64)],
    from_name: &str,
    to_name: &str,
    to_rank: u8,
) -> Option<Step> {
    if before.len() < 2 || after.len() < 2 {
        return None;
    }
    let angle = turn_angle(end_bearings(before, 15.0).1, end_bearings(after, 15.0).0);
    let turn = classify(angle);
    let slight = matches!(turn, Turn::SlightLeft | Turn::SlightRight);
    if turn == Turn::Straight || (slight && from_name == to_name) {
        return None;
    }
    let (lat, lon) = after[0];
    Some(Step {
        at_m,
        lat,
        lon,
        node_name: String::new(),
        turn,
        angle,
        from_name: from_name.to_string(),
        to_name: to_name.to_string(),
        to_rank,
        sign: sign_color(to_rank),
        branches: 2,
        clock: None,
    })
}

/// 골목 구간 하나의 좌표, 길이, LINK_ID, 도로명 구간
struct Leg {
    points: Vec<(f64, f64)>,
    length: f64,
    links: Vec<u64>,
    segments: Vec<RoadSegment>,
}

impl RouteDb {
    /// 골목 파일을 연다. 없으면 골목 없이 탐색한다
    pub fn with_alleys(mut self, path: &Path) -> io::Result<Self> {
        self.alleys = Some(AlleyFile::open(path)?);
        Ok(self)
    }

    pub fn alleys(&self) -> Option<&AlleyFile> {
        self.alleys.as_ref()
    }

    /// 좌표 p가 가장 가까운 큰길(nearest_road_m)보다 골목에 가까우면 골목망을 탐색하여 dir 방향 포털을 큰길 후보로 만든다.
    /// 후보의 distance_m은 골목 스냅 거리에 골목 길이 × factor를 더한 값이다. 방위각이 있으면 그 방향과 90° 안인 쪽으로만 출발한다
    pub fn alley_end(
        &self,
        p: (f64, f64),
        nearest_road_m: f64,
        dir: u8,
        factor: f64,
        heading: Option<f64>,
    ) -> Option<AlleyEnd> {
        let alleys = self.alleys.as_ref()?;
        let snap = alleys.snap(p.0, p.1, ALLEY_SNAP_MAX_M)?;
        if snap.distance_m + ALLEY_SNAP_MARGIN_M >= nearest_road_m {
            return None;
        }
        let forward = heading.map(|h| heading_diff(snap.bearing_deg, h) <= 90.0);
        let search = alleys.search(&snap, dir, ALLEY_SEARCH_M, ALLEY_MAX_PORTALS, forward);
        let mut candidates: Vec<(Snap, Vec<Piece>)> = Vec::new();
        for &node in &search.portal_nodes {
            let d = search.dist(node).unwrap_or(0.0);
            for (edge, frac, pd) in alleys.portals_of(node) {
                if pd & dir == 0 {
                    continue;
                }
                let cost = snap.distance_m + d * factor;
                if candidates
                    .iter()
                    .any(|(s, _)| s.edge == edge && s.distance_m <= cost)
                {
                    continue;
                }
                candidates.retain(|(s, _)| s.edge != edge);
                let points = self.edge_points(edge as usize);
                let offset_m = frac * planar_length(&points);
                let (lat, lon) = point_at(&points, offset_m);
                candidates.push((
                    Snap {
                        edge,
                        distance_m: cost,
                        lat,
                        lon,
                        offset_m,
                        bearing_deg: 0.0,
                    },
                    search.pieces_to(node),
                ));
            }
        }
        if candidates.is_empty() {
            return None;
        }
        candidates.sort_by(|a, b| a.0.distance_m.total_cmp(&b.0.distance_m));
        candidates.truncate(ALLEY_MAX_CANDIDATES);
        Some(AlleyEnd {
            snap,
            search,
            candidates,
        })
    }

    /// 출발 골목망 탐색으로 도착 골목 스냅까지 골목만 지나는 경로. (길이, 조각 열)
    pub fn alley_only(&self, from: &AlleyEnd, to: &AlleyEnd) -> Option<(f64, Vec<Piece>)> {
        let alleys = self.alleys.as_ref()?;
        let k = to.snap.link;
        let len = alleys.link_len()[k as usize] as f64;
        let fd = to.snap.frac;
        let mut best: Option<(f64, Vec<Piece>)> = None;
        if from.snap.link == k {
            best = Some((
                (from.snap.frac - fd).abs() * len,
                vec![Piece {
                    link: k,
                    from_frac: from.snap.frac,
                    to_frac: fd,
                }],
            ));
        }
        for (node, add, frac0) in [
            (alleys.link_from()[k as usize], fd * len, 0.0),
            (alleys.link_to()[k as usize], (1.0 - fd) * len, 1.0),
        ] {
            let Some(d) = from.search.dist(node) else {
                continue;
            };
            if best.as_ref().is_some_and(|b| b.0 <= d + add) {
                continue;
            }
            let mut pieces = from.search.pieces_to(node);
            pieces.push(Piece {
                link: k,
                from_frac: frac0,
                to_frac: fd,
            });
            best = Some((d + add, pieces));
        }
        best
    }

    fn leg(&self, pieces: &[Piece]) -> Leg {
        let alleys = self.alleys.as_ref().expect("골목 파일이 없음");
        let mut leg = Leg {
            points: Vec::new(),
            length: 0.0,
            links: Vec::new(),
            segments: Vec::new(),
        };
        for p in pieces {
            let len = alleys.piece_length(p);
            if len <= 0.0 {
                continue;
            }
            let pts = alleys.piece_points(p);
            leg.points
                .extend(pts.iter().skip(if leg.points.is_empty() { 0 } else { 1 }));
            leg.links.push(alleys.piece_link_id(p));
            let name = alleys.name_of(p.link);
            match leg.segments.last_mut() {
                Some(s) if s.name == name => {
                    s.to_m += len;
                    s.links += 1;
                }
                _ => leg.segments.push(RoadSegment {
                    name: name.to_string(),
                    from_m: leg.length,
                    to_m: leg.length + len,
                    links: 1,
                    rank: ALLEY_RANK,
                }),
            }
            leg.length += len;
        }
        leg
    }

    /// 큰길 경로 앞에 prefix, 뒤에 suffix 골목 조각 열(지나는 순서)을 잇는다. 거리 값은 앞 골목 길이만큼 민다.
    /// 골목에서 출발·도착하면 경로의 출발·도착 스냅은 골목 스냅이고 간선 번호는 u32::MAX, 방위각은 골목을 지나는 방향이다
    pub fn splice(
        &self,
        mut route: Route,
        prefix: &[Piece],
        suffix: &[Piece],
        from: Option<&AlleySnap>,
        to: Option<&AlleySnap>,
    ) -> Route {
        let alley_snap = |a: &AlleySnap, piece: Option<&Piece>| Snap {
            edge: u32::MAX,
            distance_m: a.distance_m,
            lat: a.lat,
            lon: a.lon,
            offset_m: 0.0,
            bearing_deg: match piece {
                Some(p) if p.to_frac < p.from_frac => (a.bearing_deg + 180.0) % 360.0,
                _ => a.bearing_deg,
            },
        };
        if let Some(a) = from {
            route.from = alley_snap(a, prefix.first());
        }
        if let Some(a) = to {
            route.to = alley_snap(a, suffix.last());
        }
        if prefix.is_empty() && suffix.is_empty() {
            return route;
        }
        let (pre, suf) = (self.leg(prefix), self.leg(suffix));
        let shift = pre.length;
        // 골목에서 큰길로, 큰길에서 골목으로 바뀌는 자리의 안내 지점
        let road_rank = route
            .links
            .first()
            .and_then(|&l| self.edge_of_link(l))
            .map_or(ALLEY_RANK, |e| self.file.road_rank()[e as usize]);
        let name = |segs: &[RoadSegment], last: bool| {
            let s = if last { segs.last() } else { segs.first() };
            s.map_or(String::new(), |s| s.name.clone())
        };
        let enter = boundary_step(
            shift,
            &pre.points,
            &route.geometry,
            &name(&pre.segments, true),
            &name(&route.segments, false),
            road_rank,
        );
        let leave = boundary_step(
            shift + route.distance_m,
            &route.geometry,
            &suf.points,
            &name(&route.segments, true),
            &name(&suf.segments, false),
            ALLEY_RANK,
        );
        for p in &mut route.pois {
            p.at_m += shift;
        }
        for t in &mut route.tolls {
            t.at_m += shift;
        }
        for s in &mut route.steps {
            s.at_m += shift;
        }
        route.steps.splice(0..0, enter);
        route.steps.extend(leave);
        for z in &mut route.school_zones {
            z.from_m += shift;
            z.to_m += shift;
        }
        let main_len = route.distance_m;
        let mut segments = pre.segments;
        segments.extend(route.segments.into_iter().map(|mut s| {
            s.from_m += shift;
            s.to_m += shift;
            s
        }));
        segments.extend(suf.segments.into_iter().map(|mut s| {
            s.from_m += shift + main_len;
            s.to_m += shift + main_len;
            s
        }));
        route.segments = segments;
        let mut links = pre.links;
        links.append(&mut route.links);
        links.extend(suf.links);
        route.links = links;
        let mut geometry = pre.points;
        let skip = usize::from(!geometry.is_empty() && !route.geometry.is_empty());
        geometry.extend(route.geometry.iter().skip(skip));
        let skip = usize::from(!geometry.is_empty() && !suf.points.is_empty());
        geometry.extend(suf.points.iter().skip(skip));
        route.geometry = geometry;
        route.distance_m += pre.length + suf.length;
        route.duration_s += (pre.length + suf.length) / (ALLEY_SPEED_KMH / 3.6);
        route
    }

    /// 골목만 지나는 경로. 출발·도착 스냅은 큰길 간선이 없으므로 간선 번호를 u32::MAX로 둔다
    pub fn alley_route(
        &self,
        profile: Profile,
        from: &AlleyEnd,
        to: &AlleyEnd,
        pieces: &[Piece],
    ) -> Route {
        let snap = |a: &AlleySnap| Snap {
            edge: u32::MAX,
            distance_m: a.distance_m,
            lat: a.lat,
            lon: a.lon,
            offset_m: 0.0,
            bearing_deg: 0.0,
        };
        let empty = Route {
            profile,
            from: snap(&from.snap),
            to: snap(&to.snap),
            distance_m: 0.0,
            duration_s: 0.0,
            links: Vec::new(),
            geometry: Vec::new(),
            tolls: Vec::new(),
            pois: Vec::new(),
            segments: Vec::new(),
            steps: Vec::new(),
            school_zones: Vec::new(),
            min_height_cm: 0,
            min_weight_kg: 0,
            rest_veh: Vec::new(),
        };
        self.splice(empty, pieces, &[], Some(&from.snap), Some(&to.snap))
    }
}
