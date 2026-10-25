//! 짧은 거리 경로. 출발·도착 직선거리가 LOCAL_ROUTE_MAX_M 이하이면 큰길 그래프(회전 규칙 포함)와 골목 파일을 한 그래프로 보고
//! 다익스트라로 탐색한다. 출발·도착 근처뿐 아니라 가는 도중의 골목도 지날 수 있다

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::alley::{AlleySnap, PORTAL_IN, PORTAL_OUT, Piece};
use crate::profile::EXPRESSWAY;
use crate::route::{
    Profile, Route, RouteDb, end_bearings, heading_diff, planar_distance, planar_length, point_at,
};
use crate::route_alley::{ALLEY_SNAP_MARGIN_M, ALLEY_SNAP_MAX_M, ALLEY_SPEED_KMH, boundary_step};
use crate::snap::Snap;

/// 짧은 거리 탐색을 하는 출발·도착 직선거리 상한(m)
pub const LOCAL_ROUTE_MAX_M: f64 = 3_000.0;
/// 큰길에서 골목으로 들어가는 포털을 모으는 범위. 출발·도착을 둘러싼 사각형에 더하는 여유(도)이며 약 1.5km이다
const LOCAL_MARGIN_DEG: f64 = 0.015;
/// 탐색을 끝내는 확정 상태 수 상한
const LOCAL_MAX_SETTLED: usize = 400_000;
/// 골목 노드 상태 키에 더하는 값. 큰길 간선 상태 키는 간선 번호이다
const ALLEY_KEY: u64 = 1 << 40;
/// 골목 간선의 역방향 비트
const REVERSED: u32 = 1 << 31;

/// 상태에 닿은 방법. 끝이라고 적은 것은 도착으로 가는 마지막 이동이다
#[derive(Clone, Copy, Debug)]
enum Move {
    /// 출발 큰길 스냅. 간선의 frac부터 끝까지
    RoadStart { snap: usize, edge: u32, frac: f64 },
    /// 출발 큰길 스냅에서 같은 간선의 골목 포털로 나감
    RoadStartToAlley {
        snap: usize,
        edge: u32,
        from: f64,
        to: f64,
    },
    /// 출발 골목 스냅에서 골목 노드까지
    AlleyStart(Piece),
    /// 큰길 통행 t
    Turn(usize),
    /// 골목 링크
    AlleyEdge(Piece),
    /// 골목 노드에서 큰길 간선의 frac으로 들어가 간선 끝까지
    PortalOut { edge: u32, frac: f64 },
    /// 큰길 통행 t로 들어간 간선의 frac에서 골목 노드로 나감
    PortalIn { turn: usize, frac: f64 },
    /// 끝. 통행 t로 들어간 간선의 frac에서 도착
    TurnTo { turn: usize, frac: f64 },
    /// 끝. 출발 큰길 스냅과 같은 간선에서 도착
    RoadDirect {
        snap: usize,
        edge: u32,
        from: f64,
        to: f64,
    },
    /// 끝. 골목 노드에서 큰길 간선으로 들어가 같은 간선에서 도착
    PortalOutTo { edge: u32, from: f64, to: f64 },
    /// 끝. 골목 노드에서 도착 골목 스냅까지
    AlleyEnd(Piece),
    /// 끝. 출발 골목 스냅과 같은 골목 링크에서 도착
    AlleyDirect(Piece),
}

#[derive(PartialEq)]
struct Item(f64, u64);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0)
    }
}

/// 경로 조각. 큰길 간선의 비율 구간 또는 골목 조각
#[derive(Clone, Copy, Debug)]
enum Seg {
    Road { edge: u32, from: f64, to: f64 },
    Alley(Piece),
}

/// 프로파일별 비용. 최단거리는 m, 최소시간·무료도로는 초이다
struct Cost<'a> {
    db: &'a RouteDb,
    profile: Profile,
    factors: Vec<f32>,
}

impl Cost<'_> {
    fn speed_cost(length: f64, max_spd: u16) -> f64 {
        length / (max_spd.max(1) as f64 / 3.6)
    }

    /// 간선 하나의 비용. 막힌 간선(배율 무한대, 무료도로의 고속국도·요금 간선)은 None
    fn edge(&self, e: u32) -> Option<f64> {
        let f = &self.db.file;
        let e = e as usize;
        if self.profile == Profile::Distance {
            return Some(f.length()[e]);
        }
        if self.profile == Profile::Free
            && (f.road_rank()[e] == EXPRESSWAY || !f.tolls_of(e).is_empty())
        {
            return None;
        }
        let factor = self.factors[e] as f64;
        if factor.is_infinite() {
            return None;
        }
        Some(Self::speed_cost(f.length()[e], f.max_spd()[e]) * factor)
    }

    /// 통행 t의 비용. 경유 간선과 들어가는 간선 전체, 건너가는 거리를 포함한다
    fn turn(&self, t: usize) -> Option<f64> {
        let f = &self.db.file;
        if self.profile == Profile::Distance {
            return Some(f.turn_weight()[t]);
        }
        let head = f.turn_head()[t];
        let via = f.via_of(t);
        let seq_len: f64 =
            via.iter().map(|&e| f.length()[e as usize]).sum::<f64>() + f.length()[head as usize];
        let gap = (f.turn_weight()[t] - seq_len).max(0.0);
        let mut total = self.edge(head)?;
        for &e in via {
            total += self.edge(e)?;
        }
        let factor = self.factors[head as usize] as f64;
        Some(total + Self::speed_cost(gap, f.max_spd()[head as usize]) * factor)
    }

    /// 골목 길이의 비용
    fn alley(&self, length: f64) -> f64 {
        if self.profile == Profile::Distance {
            length
        } else {
            length / (ALLEY_SPEED_KMH / 3.6)
        }
    }

    /// 출발·도착 좌표에서 스냅 지점까지의 거리 비용. 골목 속도로 지나는 것과 같게 본다
    fn snap(&self, distance_m: f64) -> f64 {
        self.alley(distance_m)
    }
}

/// 다익스트라 상태 묶음
struct Search {
    dist: HashMap<u64, f64>,
    prev: HashMap<u64, (Option<u64>, Move)>,
    heap: BinaryHeap<Item>,
    best: Option<(f64, Option<u64>, Move)>,
}

impl Search {
    fn relax(&mut self, key: u64, d: f64, p: Option<u64>, m: Move) {
        if self.dist.get(&key).is_none_or(|&old| d < old) {
            self.dist.insert(key, d);
            self.prev.insert(key, (p, m));
            self.heap.push(Item(d, key));
        }
    }

    fn offer(&mut self, d: f64, p: Option<u64>, m: Move) {
        if self.best.as_ref().is_none_or(|b| d < b.0) {
            self.best = Some((d, p, m));
        }
    }
}

impl RouteDb {
    /// 짧은 거리 경로. 골목 파일이 없거나 직선거리가 LOCAL_ROUTE_MAX_M를 넘거나 경로가 없으면 None이다.
    /// froms·tos는 큰길 스냅 후보이며 froms는 출발 방위각으로 거른 것이다
    pub fn local_route(
        &self,
        profile: Profile,
        from: (f64, f64),
        from_heading: Option<f64>,
        froms: &[Snap],
        to: (f64, f64),
        tos: &[Snap],
    ) -> Option<Route> {
        let alleys = self.alleys.as_ref()?;
        if planar_distance(from, to) > LOCAL_ROUTE_MAX_M || froms.is_empty() || tos.is_empty() {
            return None;
        }
        let cost = Cost {
            db: self,
            profile,
            factors: if profile == Profile::Distance {
                Vec::new()
            } else {
                self.factors()
            },
        };
        let file = &self.file;
        let link_len = alleys.link_len();
        let nearest = |s: &[Snap]| s.iter().map(|x| x.distance_m).fold(f64::INFINITY, f64::min);
        let pick = |p: (f64, f64), road_m: f64| {
            alleys
                .snap(p.0, p.1, ALLEY_SNAP_MAX_M)
                .filter(|a| a.distance_m + ALLEY_SNAP_MARGIN_M < road_m)
        };
        let from_alley = pick(from, nearest(froms));
        let to_alley = pick(to, nearest(tos));

        // 큰길 간선마다 그 간선에서 골목으로 나가는 포털 (골목 노드, 위치 비율)
        let (lat0, lat1) = (
            from.0.min(to.0) - LOCAL_MARGIN_DEG,
            from.0.max(to.0) + LOCAL_MARGIN_DEG,
        );
        let (lon0, lon1) = (
            from.1.min(to.1) - LOCAL_MARGIN_DEG,
            from.1.max(to.1) + LOCAL_MARGIN_DEG,
        );
        let mut portal_in: HashMap<u32, Vec<(u32, f64)>> = HashMap::new();
        let mut seen: HashSet<u32> = HashSet::new();
        for k in alleys.links_in(lat0, lon0, lat1, lon1) {
            for v in [alleys.link_from()[k as usize], alleys.link_to()[k as usize]] {
                if seen.insert(v) {
                    for (e, f, d) in alleys.portals_of(v) {
                        if d & PORTAL_IN != 0 {
                            portal_in.entry(e).or_default().push((v, f));
                        }
                    }
                }
            }
        }

        // 도착. 큰길 스냅은 간선마다 (위치 비율, 스냅 비용), 골목 스냅은 링크 양 끝 노드에서 (비용, 조각)
        let mut road_targets: HashMap<u32, Vec<(f64, f64)>> = HashMap::new();
        let mut alley_targets: HashMap<u32, (f64, Piece)> = HashMap::new();
        match &to_alley {
            Some(a) => {
                let len = link_len[a.link as usize] as f64;
                let pen = cost.snap(a.distance_m);
                for (node, from_frac, part) in [
                    (alleys.link_from()[a.link as usize], 0.0, a.frac),
                    (alleys.link_to()[a.link as usize], 1.0, 1.0 - a.frac),
                ] {
                    let c = cost.alley(part * len) + pen;
                    if alley_targets.get(&node).is_none_or(|x| c < x.0) {
                        let piece = Piece {
                            link: a.link,
                            from_frac,
                            to_frac: a.frac,
                        };
                        alley_targets.insert(node, (c, piece));
                    }
                }
            }
            None => {
                for s in tos {
                    road_targets
                        .entry(s.edge)
                        .or_default()
                        .push((self.fraction(s), cost.snap(s.distance_m)));
                }
            }
        }
        let targets_on = |e: u32| road_targets.get(&e).map_or(&[][..], Vec::as_slice);
        let portals_on = |e: u32| portal_in.get(&e).map_or(&[][..], Vec::as_slice);

        let mut s = Search {
            dist: HashMap::new(),
            prev: HashMap::new(),
            heap: BinaryHeap::new(),
            best: None,
        };
        // 출발 상태
        match &from_alley {
            Some(a) => {
                let len = link_len[a.link as usize] as f64;
                let pen = cost.snap(a.distance_m);
                let forward = from_heading.map(|h| heading_diff(a.bearing_deg, h) <= 90.0);
                for (node, to_frac, part) in [
                    (alleys.link_from()[a.link as usize], 0.0, a.frac),
                    (alleys.link_to()[a.link as usize], 1.0, 1.0 - a.frac),
                ] {
                    if forward.is_some_and(|f| f != (to_frac == 1.0)) {
                        continue;
                    }
                    let piece = Piece {
                        link: a.link,
                        from_frac: a.frac,
                        to_frac,
                    };
                    s.relax(
                        ALLEY_KEY | node as u64,
                        pen + cost.alley(part * len),
                        None,
                        Move::AlleyStart(piece),
                    );
                }
                if let Some(b) = &to_alley
                    && b.link == a.link
                {
                    let piece = Piece {
                        link: a.link,
                        from_frac: a.frac,
                        to_frac: b.frac,
                    };
                    let d =
                        pen + cost.snap(b.distance_m) + cost.alley((a.frac - b.frac).abs() * len);
                    s.offer(d, None, Move::AlleyDirect(piece));
                }
            }
            None => {
                for (i, snap) in froms.iter().enumerate() {
                    let Some(c) = cost.edge(snap.edge) else {
                        continue;
                    };
                    let (e, fs, pen) = (snap.edge, self.fraction(snap), cost.snap(snap.distance_m));
                    s.relax(
                        e as u64,
                        pen + (1.0 - fs) * c,
                        None,
                        Move::RoadStart {
                            snap: i,
                            edge: e,
                            frac: fs,
                        },
                    );
                    for &(fd, tpen) in targets_on(e) {
                        if fd >= fs {
                            let m = Move::RoadDirect {
                                snap: i,
                                edge: e,
                                from: fs,
                                to: fd,
                            };
                            s.offer(pen + (fd - fs) * c + tpen, None, m);
                        }
                    }
                    for &(v, f) in portals_on(e) {
                        if f >= fs {
                            let m = Move::RoadStartToAlley {
                                snap: i,
                                edge: e,
                                from: fs,
                                to: f,
                            };
                            s.relax(ALLEY_KEY | v as u64, pen + (f - fs) * c, None, m);
                        }
                    }
                }
            }
        }

        // 다익스트라
        let (turn_first_out, turn_head) = (file.turn_first_out(), file.turn_head());
        let (first_out, head, edge_link) = (alleys.first_out(), alleys.head(), alleys.edge_link());
        let mut done: HashSet<u64> = HashSet::new();
        while let Some(Item(d, key)) = s.heap.pop() {
            if s.best.as_ref().is_some_and(|b| d >= b.0) || done.len() >= LOCAL_MAX_SETTLED {
                break;
            }
            if !done.insert(key) {
                continue;
            }
            if key < ALLEY_KEY {
                let a = key as usize;
                let turns = turn_first_out[a] as usize..turn_first_out[a + 1] as usize;
                for (t, &e) in turn_head
                    .iter()
                    .enumerate()
                    .take(turns.end)
                    .skip(turns.start)
                {
                    let Some(c) = cost.turn(t) else {
                        continue;
                    };
                    let nd = d + c;
                    s.relax(e as u64, nd, Some(key), Move::Turn(t));
                    let ce = cost.edge(e).unwrap_or(0.0);
                    let start = nd - ce;
                    for &(fd, tpen) in targets_on(e) {
                        let m = Move::TurnTo { turn: t, frac: fd };
                        s.offer(start + fd * ce + tpen, Some(key), m);
                    }
                    for &(v, f) in portals_on(e) {
                        let m = Move::PortalIn { turn: t, frac: f };
                        s.relax(ALLEY_KEY | v as u64, start + f * ce, Some(key), m);
                    }
                }
            } else {
                let u = (key - ALLEY_KEY) as u32;
                if let Some(&(c, piece)) = alley_targets.get(&u) {
                    s.offer(d + c, Some(key), Move::AlleyEnd(piece));
                }
                for i in first_out[u as usize] as usize..first_out[u as usize + 1] as usize {
                    let raw = edge_link[i];
                    let l = raw & !REVERSED;
                    let (from_frac, to_frac) = if raw & REVERSED != 0 {
                        (1.0, 0.0)
                    } else {
                        (0.0, 1.0)
                    };
                    let piece = Piece {
                        link: l,
                        from_frac,
                        to_frac,
                    };
                    let nd = d + cost.alley(link_len[l as usize] as f64);
                    s.relax(
                        ALLEY_KEY | head[i] as u64,
                        nd,
                        Some(key),
                        Move::AlleyEdge(piece),
                    );
                }
                for (e, f, pd) in alleys.portals_of(u) {
                    if pd & PORTAL_OUT == 0 {
                        continue;
                    }
                    let Some(c) = cost.edge(e) else {
                        continue;
                    };
                    let m = Move::PortalOut { edge: e, frac: f };
                    s.relax(e as u64, d + (1.0 - f) * c, Some(key), m);
                    for &(fd, tpen) in targets_on(e) {
                        if fd >= f {
                            let m = Move::PortalOutTo {
                                edge: e,
                                from: f,
                                to: fd,
                            };
                            s.offer(d + (fd - f) * c + tpen, Some(key), m);
                        }
                    }
                }
            }
        }

        // 이동 열을 조각 열로 바꾼다
        let (_, last_key, last_move) = s.best?;
        let mut moves = vec![last_move];
        let mut cur = last_key;
        while let Some(k) = cur {
            let (p, m) = s.prev[&k];
            moves.push(m);
            cur = p;
        }
        moves.reverse();
        let mut segs: Vec<Seg> = Vec::new();
        let mut start_snap = None;
        let road_seg = |edge: u32, from: f64, to: f64| Seg::Road { edge, from, to };
        let vias = |segs: &mut Vec<Seg>, t: usize| {
            for &v in file.via_of(t) {
                segs.push(road_seg(v, 0.0, 1.0));
            }
        };
        for m in &moves {
            match *m {
                Move::RoadStart { snap, edge, frac } => {
                    start_snap = Some(snap);
                    segs.push(road_seg(edge, frac, 1.0));
                }
                Move::RoadStartToAlley {
                    snap,
                    edge,
                    from,
                    to,
                }
                | Move::RoadDirect {
                    snap,
                    edge,
                    from,
                    to,
                } => {
                    start_snap = Some(snap);
                    segs.push(road_seg(edge, from, to));
                }
                Move::AlleyStart(p)
                | Move::AlleyEdge(p)
                | Move::AlleyEnd(p)
                | Move::AlleyDirect(p) => segs.push(Seg::Alley(p)),
                Move::Turn(t) => {
                    vias(&mut segs, t);
                    segs.push(road_seg(turn_head[t], 0.0, 1.0));
                }
                Move::PortalOut { edge, frac } => segs.push(road_seg(edge, frac, 1.0)),
                Move::PortalIn { turn, frac } | Move::TurnTo { turn, frac } => {
                    vias(&mut segs, turn);
                    segs.push(road_seg(turn_head[turn], 0.0, frac));
                }
                Move::PortalOutTo { edge, from, to } => segs.push(road_seg(edge, from, to)),
            }
        }
        // 큰길에서 도착하면 마지막 조각과 같은 간선·비율의 도착 스냅을 경로의 도착으로 한다
        let end_snap = match segs.last() {
            Some(Seg::Road { edge, to, .. }) if to_alley.is_none() => tos
                .iter()
                .find(|t| t.edge == *edge && (self.fraction(t) - to).abs() < 1e-9),
            _ => None,
        };
        Some(self.local_build(
            profile,
            &segs,
            from_alley.as_ref(),
            start_snap.map(|i| &froms[i]),
            to_alley.as_ref(),
            end_snap,
        ))
    }

    /// 조각 열로 Route를 만든다. 큰길 조각이 이어지는 구간마다 build로 만들고 골목 구간과 잇는다
    fn local_build(
        &self,
        profile: Profile,
        segs: &[Seg],
        from_alley: Option<&AlleySnap>,
        from_road: Option<&Snap>,
        to_alley: Option<&AlleySnap>,
        to_road: Option<&Snap>,
    ) -> Route {
        let length = self.file.length();
        let snap_at = |edge: u32, frac: f64| {
            let pts = self.edge_points(edge as usize);
            let planar = planar_length(&pts);
            let (lat, lon) = point_at(&pts, frac * planar);
            Snap {
                edge,
                distance_m: 0.0,
                lat,
                lon,
                offset_m: frac * planar,
                bearing_deg: end_bearings(&pts, 15.0).0,
            }
        };
        // (골목 구간 여부, 경로)
        let mut parts: Vec<(bool, Route)> = Vec::new();
        let mut i = 0;
        while i < segs.len() {
            if let Seg::Alley(_) = segs[i] {
                let mut pieces = Vec::new();
                while let Some(Seg::Alley(p)) = segs.get(i) {
                    pieces.push(*p);
                    i += 1;
                }
                let empty = self.empty_route(profile, snap_at(0, 0.0), snap_at(0, 0.0));
                parts.push((true, self.splice(empty, &pieces, &[], None, None)));
                continue;
            }
            let mut block: Vec<(u32, f64, f64)> = Vec::new();
            while let Some(Seg::Road { edge, from, to }) = segs.get(i) {
                block.push((*edge, *from, *to));
                i += 1;
            }
            let (first, last) = (block[0], block[block.len() - 1]);
            let dist: f64 = block
                .iter()
                .map(|&(e, f, t)| length[e as usize] * (t - f))
                .sum();
            let edges: Vec<u32> = block.iter().map(|b| b.0).collect();
            let a = match from_road {
                Some(s) if parts.is_empty() => s.clone(),
                _ => snap_at(first.0, first.1),
            };
            let b = match to_road {
                Some(s) if i >= segs.len() => s.clone(),
                _ => snap_at(last.0, last.2),
            };
            parts.push((false, self.build(profile, a, b, dist, &edges)));
        }
        // 골목과 큰길이 바뀌는 자리의 안내 지점
        let mut boundary = Vec::new();
        let mut walked = 0.0;
        for w in parts.windows(2) {
            let ((a_alley, a), (b_alley, b)) = (&w[0], &w[1]);
            walked += a.distance_m;
            if a_alley == b_alley {
                continue;
            }
            let to_rank = if *b_alley {
                108
            } else {
                b.links
                    .first()
                    .and_then(|&l| self.edge_of_link(l))
                    .map_or(108, |e| self.file.road_rank()[e as usize])
            };
            let from_name = a.segments.last().map_or("", |s| s.name.as_str());
            let to_name = b.segments.first().map_or("", |s| s.name.as_str());
            boundary.extend(boundary_step(
                walked,
                &a.geometry,
                &b.geometry,
                from_name,
                to_name,
                to_rank,
            ));
        }
        let mut route = parts
            .into_iter()
            .map(|p| p.1)
            .reduce(concat)
            .expect("경로 조각이 없음");
        route.steps.extend(boundary);
        route.steps.sort_by(|x, y| x.at_m.total_cmp(&y.at_m));
        let alley_snap = |a: &AlleySnap, p: &Piece| Snap {
            edge: u32::MAX,
            distance_m: a.distance_m,
            lat: a.lat,
            lon: a.lon,
            offset_m: 0.0,
            bearing_deg: if p.to_frac < p.from_frac {
                (a.bearing_deg + 180.0) % 360.0
            } else {
                a.bearing_deg
            },
        };
        if let (Some(a), Some(Seg::Alley(p))) = (from_alley, segs.first()) {
            route.from = alley_snap(a, p);
        }
        if let (Some(a), Some(Seg::Alley(p))) = (to_alley, segs.last()) {
            route.to = alley_snap(a, p);
        }
        route
    }

    /// 거리 0인 빈 경로
    pub(crate) fn empty_route(&self, profile: Profile, from: Snap, to: Snap) -> Route {
        Route {
            profile,
            from,
            to,
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
        }
    }
}

/// 두 경로를 잇는다. 뒤 경로의 거리 값은 앞 경로 길이만큼 민다
fn concat(mut a: Route, b: Route) -> Route {
    let shift = a.distance_m;
    a.pois.extend(b.pois.into_iter().map(|mut p| {
        p.at_m += shift;
        p
    }));
    a.tolls.extend(b.tolls.into_iter().map(|mut t| {
        t.at_m += shift;
        t
    }));
    a.steps.extend(b.steps.into_iter().map(|mut s| {
        s.at_m += shift;
        s
    }));
    a.school_zones
        .extend(b.school_zones.into_iter().map(|mut z| {
            z.from_m += shift;
            z.to_m += shift;
            z
        }));
    for mut seg in b.segments {
        seg.from_m += shift;
        seg.to_m += shift;
        match a.segments.last_mut() {
            Some(l) if l.name == seg.name && l.rank == seg.rank => {
                l.to_m = seg.to_m;
                l.links += seg.links;
            }
            _ => a.segments.push(seg),
        }
    }
    a.links.extend(b.links);
    let skip = usize::from(!a.geometry.is_empty() && !b.geometry.is_empty());
    a.geometry.extend(b.geometry.into_iter().skip(skip));
    a.distance_m += b.distance_m;
    a.duration_s += b.duration_s;
    let min_nonzero = |x: u16, y: u16| match (x, y) {
        (0, y) => y,
        (x, 0) => x,
        (x, y) => x.min(y),
    };
    a.min_height_cm = min_nonzero(a.min_height_cm, b.min_height_cm);
    a.min_weight_kg = min_nonzero(a.min_weight_kg, b.min_weight_kg);
    for v in b.rest_veh {
        if !a.rest_veh.contains(&v) {
            a.rest_veh.push(v);
        }
    }
    a.to = b.to;
    a
}
