use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// 링크를 노드로, 링크에서 링크로 이어지는 통행을 간선으로 하는 line graph
pub struct LineGraph {
    /// 링크마다 이어지는 첫 통행 번호
    pub first_out: Vec<u32>,
    /// 통행마다 다음 링크 번호
    pub head: Vec<u32>,
    /// 통행마다 비용(m). 교차로 내부 링크 길이와 다음 링크 길이의 합이다.
    pub weight: Vec<f64>,
    /// 통행마다 경유 내부 링크가 시작하는 번호
    pub via_first: Vec<u32>,
    /// 경유 내부 링크 번호
    pub via: Vec<u32>,
}

/// 회전 규칙. st 링크로 들어와 ed 링크로 나가는 통행이다.
pub struct TurnRule {
    pub st: u32,
    pub ed: u32,
}

/// line graph 생성 입력
pub struct LineGraphInput<'a> {
    pub first_out: &'a [u32],
    pub head: &'a [u32],
    pub length: &'a [f64],
    /// 노드마다 교차로 그룹 번호. 그룹이 없으면 u32::MAX
    pub group: &'a [u32],
    /// 간선마다 교차로 내부 링크 여부
    pub internal: &'a [bool],
    /// 간선마다 링크 좌표가 시작하는 번호
    pub geom_first: &'a [u32],
    /// 링크 좌표의 평면 좌표(m)
    pub geom_xy: &'a [(f64, f64)],
    /// 간선마다 시작 방향 단위 벡터
    pub start_dir: &'a [(f64, f64)],
    /// 간선마다 끝 방향 단위 벡터
    pub end_dir: &'a [(f64, f64)],
    /// 금지 회전
    pub forbidden: &'a [TurnRule],
    /// 허용 U턴
    pub uturn: &'a [TurnRule],
    /// 진입 링크와 진출 링크를 잇는 링크가 없는 허용 U턴. 진입 링크 끝에서 진출 링크 시작으로 바로 건너가는 통행을 만든다
    pub bridges: &'a [TurnRule],
}

/// line graph 생성 통계
#[derive(Debug, Default, PartialEq)]
pub struct LineGraphStats {
    /// 통행 수
    pub transitions: usize,
    /// 내부 링크를 경유하는 통행 수
    pub expanded: usize,
    /// 금지 회전으로 뺀 통행 수
    pub forbidden_applied: usize,
    /// 허용한 U턴 통행 수
    pub uturn_allowed: usize,
    /// 뺀 U턴 통행 수
    pub uturn_removed: usize,
    /// 다른 진출 링크가 없어 허용한 U턴 통행 수
    pub uturn_deadend: usize,
    /// 금지 회전에 걸렸지만 허용 U턴이 우선하여 남긴 통행 수
    pub forbidden_overridden: usize,
    /// 진입 링크 끝에서 진출 링크 시작으로 바로 건너가는 통행 수
    pub bridged: usize,
    /// 후보 통행에 한 번이라도 걸린 금지 회전 수
    pub forbidden_rules_hit: usize,
    /// 후보 통행에 한 번이라도 걸린 허용 U턴 수
    pub uturn_rules_hit: usize,
    /// 후보 통행에 걸리지 않은 금지 회전 번호
    pub forbidden_missed: Vec<usize>,
}

#[derive(PartialEq)]
struct State {
    dist: f64,
    node: u32,
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other.dist.total_cmp(&self.dist)
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 위도 1도의 길이(m)
const M_PER_LAT: f64 = 111_320.0;
/// 위도 36도에서 경도 1도의 길이(m)
const M_PER_LON: f64 = 111_320.0 * 0.809_017;

/// 위경도를 평면 좌표(m)로 변환한다.
pub fn planar(lat: &[f64], lon: &[f64]) -> Vec<(f64, f64)> {
    lat.iter()
        .zip(lon)
        .map(|(&la, &lo)| (lo * M_PER_LON, la * M_PER_LAT))
        .collect()
}

/// 점 p에서 선분 ab까지의 거리
fn point_segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// 점 p에서 좌표 목록 line까지의 거리
fn point_line_distance(p: (f64, f64), line: &[(f64, f64)]) -> f64 {
    line.windows(2)
        .map(|w| point_segment_distance(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

/// 간선마다 링크 좌표의 첫 구간과 마지막 구간 방향을 단위 벡터로 만든다. 좌표가 두 점 미만이면 (0, 0)이다.
pub fn link_directions(
    geom_first: &[u32],
    geom_lat: &[f64],
    geom_lon: &[f64],
) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let unit = |(la0, lo0): (f64, f64), (la1, lo1): (f64, f64)| {
        let (dx, dy) = ((lo1 - lo0) * M_PER_LON, (la1 - la0) * M_PER_LAT);
        let len = dx.hypot(dy);
        if len == 0.0 {
            (0.0, 0.0)
        } else {
            (dx / len, dy / len)
        }
    };
    let mut start = Vec::with_capacity(geom_first.len() - 1);
    let mut end = Vec::with_capacity(geom_first.len() - 1);
    for e in 0..geom_first.len() - 1 {
        let (s, t) = (geom_first[e] as usize, geom_first[e + 1] as usize);
        if t - s < 2 {
            start.push((0.0, 0.0));
            end.push((0.0, 0.0));
            continue;
        }
        start.push(unit(
            (geom_lat[s], geom_lon[s]),
            (geom_lat[s + 1], geom_lon[s + 1]),
        ));
        end.push(unit(
            (geom_lat[t - 2], geom_lon[t - 2]),
            (geom_lat[t - 1], geom_lon[t - 1]),
        ));
    }
    (start, end)
}

/// 진입 링크 a에서 진출 링크 b로의 통행이 U턴인지 판정한다.
/// a의 끝 방향과 b의 시작 방향의 코사인이 -0.9 미만이고, b의 끝점에서 a의 좌표 목록까지 또는 a의 시작점에서 b의 좌표 목록까지의 거리가 50m 이내이면 U턴이다.
pub fn is_uturn(input: &LineGraphInput, a: u32, b: u32) -> bool {
    let (ex, ey) = input.end_dir[a as usize];
    let (sx, sy) = input.start_dir[b as usize];
    if ex * sx + ey * sy >= -0.9 {
        return false;
    }
    let line = |e: u32| {
        &input.geom_xy
            [input.geom_first[e as usize] as usize..input.geom_first[e as usize + 1] as usize]
    };
    let (la, lb) = (line(a), line(b));
    if la.is_empty() || lb.is_empty() {
        return false;
    }
    point_line_distance(lb[lb.len() - 1], la).min(point_line_distance(la[0], lb)) <= 50.0
}

/// 회전 규칙을 st 링크별 (ed 링크, 규칙 번호) 목록으로 만든다.
struct Rules {
    by_st: HashMap<u32, Vec<(u32, usize)>>,
    /// 규칙마다 후보 통행에 걸렸는지 여부
    matched: Vec<Cell<bool>>,
}

impl Rules {
    fn new(rules: &[TurnRule]) -> Self {
        let mut by_st: HashMap<u32, Vec<(u32, usize)>> = HashMap::new();
        for (i, r) in rules.iter().enumerate() {
            by_st.entry(r.st).or_default().push((r.ed, i));
        }
        Self {
            by_st,
            matched: (0..rules.len()).map(|_| Cell::new(false)).collect(),
        }
    }

    /// 링크 열에서 st 뒤에 ed가 오는 규칙이 있는지 확인하고 걸린 규칙을 표시한다.
    fn hit(&self, seq: &[u32]) -> bool {
        let mut found = false;
        for (i, s) in seq.iter().enumerate() {
            if let Some(eds) = self.by_st.get(s) {
                for &(ed, r) in eds {
                    if seq[i + 1..].contains(&ed) {
                        self.matched[r].set(true);
                        found = true;
                    }
                }
            }
        }
        found
    }

    fn matched_count(&self) -> usize {
        self.matched.iter().filter(|m| m.get()).count()
    }
}

impl LineGraph {
    /// 교차로 그래프로 line graph를 만든다.
    /// 교차로 그룹으로 들어오는 링크는 그룹 내부 링크만 거쳐 닿는 진출 링크마다 통행을 만들고, 그 밖의 링크는 도착 교차로에서 출발하는 링크마다 통행을 만든다.
    /// 금지 회전의 st가 진입·경유 링크에 있고 ed가 경유·진출 링크에 있는 통행은 빼고, U턴 통행은 허용 U턴에 있거나 다른 진출 통행이 없을 때만 만든다.
    /// 허용 U턴이 걸린 통행은 금지 회전에 걸려도 남긴다.
    pub fn build(input: &LineGraphInput) -> (Self, LineGraphStats) {
        let (first_out, head, length) = (input.first_out, input.head, input.length);
        let node_count = first_out.len() - 1;
        let tail = tails(first_out);
        let forbidden = Rules::new(input.forbidden);
        let uturn = Rules::new(input.uturn);
        let mut stats = LineGraphStats::default();
        let mut line_graph = LineGraph {
            first_out: vec![0],
            head: Vec::new(),
            weight: Vec::new(),
            via_first: vec![0],
            via: Vec::new(),
        };

        let mut dist = vec![f64::INFINITY; node_count];
        let mut came_by = vec![u32::MAX; node_count];
        let mut reached: Vec<u32> = Vec::new();
        let mut heap = BinaryHeap::new();
        // 진입 링크마다 [진입, 경유.., 진출] 링크 열과 건너가는 거리(m) 후보
        let mut candidates: Vec<(Vec<u32>, f64)> = Vec::new();
        let mut bridges: HashMap<u32, Vec<u32>> = HashMap::new();
        for r in input.bridges {
            bridges.entry(r.st).or_default().push(r.ed);
        }
        let line = |e: u32| {
            &input.geom_xy
                [input.geom_first[e as usize] as usize..input.geom_first[e as usize + 1] as usize]
        };

        for a in 0..head.len() {
            candidates.clear();
            let v = head[a];
            if input.internal[a] {
                // 내부 링크에서 출발하는 통행은 없다
            } else if input.group[v as usize] == u32::MAX {
                for b in first_out[v as usize]..first_out[v as usize + 1] {
                    candidates.push((vec![a as u32, b], 0.0));
                }
            } else {
                // 그룹 내부 링크만 따라가는 다익스트라
                reached.clear();
                dist[v as usize] = 0.0;
                reached.push(v);
                heap.push(State { dist: 0.0, node: v });
                while let Some(State { dist: d, node: u }) = heap.pop() {
                    if d > dist[u as usize] {
                        continue;
                    }
                    for e in first_out[u as usize] as usize..first_out[u as usize + 1] as usize {
                        if !input.internal[e] {
                            continue;
                        }
                        let w = head[e];
                        let nd = d + length[e];
                        if nd < dist[w as usize] {
                            if dist[w as usize].is_infinite() {
                                reached.push(w);
                            }
                            dist[w as usize] = nd;
                            came_by[w as usize] = e as u32;
                            heap.push(State { dist: nd, node: w });
                        }
                    }
                }
                for &u in &reached {
                    // 진입 링크, u까지의 내부 링크 열
                    let mut seq = vec![a as u32];
                    let mut x = u;
                    while x != v {
                        let e = came_by[x as usize];
                        seq.push(e);
                        x = tail[e as usize];
                    }
                    seq[1..].reverse();
                    for b in first_out[u as usize]..first_out[u as usize + 1] {
                        if !input.internal[b as usize] {
                            let mut seq = seq.clone();
                            seq.push(b);
                            candidates.push((seq, 0.0));
                        }
                    }
                }
                for &u in &reached {
                    dist[u as usize] = f64::INFINITY;
                    came_by[u as usize] = u32::MAX;
                }
            }
            for &ed in bridges.get(&(a as u32)).into_iter().flatten() {
                let (la, lb) = (line(a as u32), line(ed));
                let gap = match (la.last(), lb.first()) {
                    (Some(p), Some(q)) => (p.0 - q.0).hypot(p.1 - q.1),
                    _ => 0.0,
                };
                candidates.push((vec![a as u32, ed], gap));
                stats.bridged += 1;
            }
            line_graph.add_entry(&candidates, input, &forbidden, &uturn, &mut stats);
            line_graph.first_out.push(line_graph.head.len() as u32);
        }
        stats.transitions = line_graph.head.len();
        stats.forbidden_rules_hit = forbidden.matched_count();
        stats.uturn_rules_hit = uturn.matched_count();
        stats.forbidden_missed = (0..input.forbidden.len())
            .filter(|&i| !forbidden.matched[i].get())
            .collect();
        (line_graph, stats)
    }

    /// 한 진입 링크의 후보 링크 열로 통행을 추가한다.
    fn add_entry(
        &mut self,
        candidates: &[(Vec<u32>, f64)],
        input: &LineGraphInput,
        forbidden: &Rules,
        uturn: &Rules,
        stats: &mut LineGraphStats,
    ) {
        let is_uturn = |seq: &[u32]| is_uturn(input, seq[0], seq[seq.len() - 1]);
        // 금지가 아니고 U턴도 아닌 통행이 있는지
        // 금지가 아니고 U턴도 아닌 통행이 있는지. 허용 U턴이 걸린 통행은 금지보다 우선한다
        let has_other = candidates
            .iter()
            .any(|(seq, _)| (uturn.hit(seq) || !forbidden.hit(seq)) && !is_uturn(seq));
        for (seq, gap) in candidates {
            let allowed = uturn.hit(seq);
            if forbidden.hit(seq) {
                if !allowed {
                    stats.forbidden_applied += 1;
                    continue;
                }
                stats.forbidden_overridden += 1;
            }
            if is_uturn(seq) {
                if allowed {
                    stats.uturn_allowed += 1;
                } else if has_other {
                    stats.uturn_removed += 1;
                    continue;
                } else {
                    stats.uturn_deadend += 1;
                }
            }
            if seq.len() > 2 {
                stats.expanded += 1;
            }
            self.head.push(seq[seq.len() - 1]);
            self.weight.push(
                gap + seq[1..]
                    .iter()
                    .map(|&e| input.length[e as usize])
                    .sum::<f64>(),
            );
            self.via.extend_from_slice(&seq[1..seq.len() - 1]);
            self.via_first.push(self.via.len() as u32);
        }
    }

    pub fn node_count(&self) -> usize {
        self.first_out.len() - 1
    }

    pub fn edge_count(&self) -> usize {
        self.head.len()
    }

    /// 통행 t가 경유하는 내부 링크 번호
    pub fn via_of(&self, t: usize) -> &[u32] {
        &self.via[self.via_first[t] as usize..self.via_first[t + 1] as usize]
    }
}

/// st 링크의 도착 노드에서 ed 링크의 출발 노드까지 max_links개 이하 링크로 가는 가장 짧은 경로의 노드 목록을 찾는다.
pub fn rule_path(
    first_out: &[u32],
    head: &[u32],
    length: &[f64],
    tail: &[u32],
    st: u32,
    ed: u32,
    max_links: usize,
) -> Option<Vec<u32>> {
    fn dfs(
        first_out: &[u32],
        head: &[u32],
        length: &[f64],
        to: u32,
        max_links: usize,
        path: &mut Vec<u32>,
        len: f64,
        best: &mut Option<(f64, Vec<u32>)>,
    ) {
        let v = *path.last().unwrap();
        if v == to {
            if best.as_ref().is_none_or(|b| len < b.0) {
                *best = Some((len, path.clone()));
            }
            return;
        }
        if path.len() > max_links || best.as_ref().is_some_and(|b| len >= b.0) {
            return;
        }
        for e in first_out[v as usize] as usize..first_out[v as usize + 1] as usize {
            let w = head[e];
            if !path.contains(&w) {
                path.push(w);
                dfs(
                    first_out,
                    head,
                    length,
                    to,
                    max_links,
                    path,
                    len + length[e],
                    best,
                );
                path.pop();
            }
        }
    }
    let mut best = None;
    dfs(
        first_out,
        head,
        length,
        tail[ed as usize],
        max_links,
        &mut vec![head[st as usize]],
        0.0,
        &mut best,
    );
    best.map(|b| b.1)
}

/// 교차로 노드 사이 max_len 이하 링크로 이어진 노드를 한 교차로 그룹으로 묶고, joined의 노드 목록마다 한 그룹으로 묶는다.
/// 노드마다 그룹 번호를 반환하며 두 개 이상 묶이지 않은 노드는 u32::MAX이다.
pub fn intersection_groups(
    first_out: &[u32],
    head: &[u32],
    length: &[f64],
    crossing: &[bool],
    max_len: f64,
    joined: &[Vec<u32>],
) -> Vec<u32> {
    let n = first_out.len() - 1;
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn find(parent: &mut [u32], x: u32) -> u32 {
        let mut r = x;
        while parent[r as usize] != r {
            r = parent[r as usize];
        }
        let mut x = x;
        while parent[x as usize] != r {
            let next = parent[x as usize];
            parent[x as usize] = r;
            x = next;
        }
        r
    }
    for v in 0..n {
        if !crossing[v] {
            continue;
        }
        for e in first_out[v] as usize..first_out[v + 1] as usize {
            let w = head[e] as usize;
            if crossing[w] && length[e] <= max_len {
                let (a, b) = (find(&mut parent, v as u32), find(&mut parent, w as u32));
                if a != b {
                    parent[a as usize] = b;
                }
            }
        }
    }
    for nodes in joined {
        for w in nodes.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            if a != b {
                parent[a as usize] = b;
            }
        }
    }
    let roots: Vec<u32> = (0..n as u32).map(|v| find(&mut parent, v)).collect();
    let mut size = vec![0u32; n];
    for &r in &roots {
        size[r as usize] += 1;
    }
    let mut group_of_root = vec![u32::MAX; n];
    let mut count = 0;
    for r in 0..n {
        if size[r] >= 2 {
            group_of_root[r] = count;
            count += 1;
        }
    }
    roots.iter().map(|&r| group_of_root[r as usize]).collect()
}

/// 간선마다 양 끝 노드가 같은 교차로 그룹에 속하는지 표시한다.
pub fn internal_edges(first_out: &[u32], head: &[u32], group: &[u32]) -> Vec<bool> {
    let tail = tails(first_out);
    (0..head.len())
        .map(|e| {
            group[tail[e] as usize] != u32::MAX
                && group[tail[e] as usize] == group[head[e] as usize]
        })
        .collect()
}

/// 간선마다 출발 교차로 번호를 만든다.
pub fn tails(first_out: &[u32]) -> Vec<u32> {
    let mut tail = Vec::with_capacity(*first_out.last().unwrap_or(&0) as usize);
    for v in 0..first_out.len() - 1 {
        for _ in first_out[v]..first_out[v + 1] {
            tail.push(v as u32);
        }
    }
    tail
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Graph, LinkInput, NodeInput};

    /// 평면 좌표(m)를 위경도로 변환한다.
    fn ll(x: f64, y: f64) -> (f64, f64) {
        (y / M_PER_LAT, x / M_PER_LON)
    }

    /// 좌표가 없는 링크에 출발·도착 교차로 좌표를 넣는다.
    fn with_geometry(nodes: &[NodeInput], mut links: Vec<LinkInput>) -> Vec<LinkInput> {
        for l in &mut links {
            if l.geometry.is_empty() {
                let at = |id| {
                    nodes
                        .iter()
                        .find(|n| n.id == id)
                        .map(|n| (n.lat, n.lon))
                        .expect("링크 끝 교차로가 목록에 없음")
                };
                l.geometry = vec![at(l.from), at(l.to)];
            }
        }
        links
    }

    struct Demo {
        g: Graph,
        geom_xy: Vec<(f64, f64)>,
        start: Vec<(f64, f64)>,
        end: Vec<(f64, f64)>,
    }

    impl Demo {
        fn new(nodes: &[NodeInput], links: Vec<LinkInput>) -> Self {
            let g = Graph::build(nodes, &with_geometry(nodes, links)).0;
            let geom_xy = planar(&g.geom_lat, &g.geom_lon);
            let (start, end) = link_directions(&g.geom_first, &g.geom_lat, &g.geom_lon);
            Self {
                g,
                geom_xy,
                start,
                end,
            }
        }

        fn input<'a>(
            &'a self,
            group: &'a [u32],
            internal: &'a [bool],
            forbidden: &'a [TurnRule],
            uturn: &'a [TurnRule],
        ) -> LineGraphInput<'a> {
            LineGraphInput {
                first_out: &self.g.first_out,
                head: &self.g.head,
                length: &self.g.length,
                group,
                internal,
                geom_first: &self.g.geom_first,
                geom_xy: &self.geom_xy,
                start_dir: &self.start,
                end_dir: &self.end,
                forbidden,
                uturn,
                bridges: &[],
            }
        }

        fn edge(&self, id: u64) -> u32 {
            self.g.link_id.iter().position(|&x| x == id).unwrap() as u32
        }

        /// 진입 링크에서 이어지는 (진출 링크 LINK_ID, 비용, 경유 LINK_ID 목록)
        fn outs(&self, line_graph: &LineGraph, entry: u64) -> Vec<(u64, f64, Vec<u64>)> {
            let a = self.edge(entry) as usize;
            (line_graph.first_out[a] as usize..line_graph.first_out[a + 1] as usize)
                .map(|t| {
                    (
                        self.g.link_id[line_graph.head[t] as usize],
                        line_graph.weight[t],
                        line_graph
                            .via_of(t)
                            .iter()
                            .map(|&v| self.g.link_id[v as usize])
                            .collect(),
                    )
                })
                .collect()
        }
    }

    #[test]
    fn build_demo_line_graph() {
        // 가(0) 나(1) 다(2) 라(3) 마(4). 가↔나, 나↔다는 양방향이고 가→라, 라→다, 다→마는 일방통행이다
        let nodes = [
            NodeInput::new(1000000101, 37.000, 127.000),
            NodeInput::new(1000000201, 37.000, 127.001),
            NodeInput::new(1000000301, 37.001, 127.001),
            NodeInput::new(1000000401, 37.001, 127.000),
            NodeInput::new(1000000501, 37.002, 127.001),
            NodeInput::new(1000000601, 37.003, 127.001),
        ];
        let links = vec![
            LinkInput::new(1000001000, 1000000101, 1000000201, 100.0),
            LinkInput::new(1000001100, 1000000201, 1000000101, 100.0),
            LinkInput::new(1000002000, 1000000201, 1000000301, 100.0),
            LinkInput::new(1000002100, 1000000301, 1000000201, 100.0),
            LinkInput::new(1000003000, 1000000101, 1000000401, 50.0),
            LinkInput::new(1000004000, 1000000401, 1000000301, 80.0),
            LinkInput::new(1000005000, 1000000301, 1000000501, 100.0),
            LinkInput::new(1000006000, 1000000501, 1000000601, 100.0),
            LinkInput::new(1000006100, 1000000601, 1000000501, 100.0),
        ];
        let demo = Demo::new(&nodes, links);
        // 간선 번호: 0 가→나, 1 가→라, 2 나→가, 3 나→다, 4 다→나, 5 다→마, 6 라→다, 7 마→바, 8 바→마
        let group = vec![u32::MAX; 6];
        let internal = vec![false; 9];
        let (line_graph, stats) = LineGraph::build(&demo.input(&group, &internal, &[], &[]));
        assert_eq!(line_graph.node_count(), 9);
        let next = |a: u32| {
            line_graph.head[line_graph.first_out[a as usize] as usize
                ..line_graph.first_out[a as usize + 1] as usize]
                .to_vec()
        };
        // 가→나 다음은 나→다뿐이고 되돌아가는 나→가는 없다
        assert_eq!(next(0), vec![3]);
        // 가→라 다음은 라→다
        assert_eq!(next(1), vec![6]);
        // 나→가 다음은 가→라뿐이다
        assert_eq!(next(2), vec![1]);
        // 나→다 다음은 다→마뿐이고 다→나는 없다
        assert_eq!(next(3), vec![5]);
        // 다→나 다음은 나→가뿐이고 나→다는 없다
        assert_eq!(next(4), vec![2]);
        // 라→다 다음은 다→나, 다→마
        assert_eq!(next(6), vec![4, 5]);
        // 다→마 다음은 마→바
        assert_eq!(next(5), vec![7]);
        // 마→바 다음은 바→마뿐이고 바→마 다음은 마→바뿐이다. 막다른 길이라 U턴을 허용한다
        assert_eq!(next(7), vec![8]);
        assert_eq!(next(8), vec![7]);
        assert_eq!(line_graph.edge_count(), 10);
        assert_eq!(stats.uturn_removed, 4);
        assert_eq!(stats.uturn_deadend, 2);
        // 통행 비용은 다음 링크 길이
        assert_eq!(line_graph.weight[line_graph.first_out[0] as usize], 100.0);

        // 가→나 뒤 나→다 금지. 다른 진출 통행이 없으므로 나→가 U턴을 허용한다
        let forbidden = [TurnRule { st: 0, ed: 3 }];
        let (line_graph, stats) = LineGraph::build(&demo.input(&group, &internal, &forbidden, &[]));
        assert_eq!(
            line_graph.head[line_graph.first_out[0] as usize..line_graph.first_out[1] as usize],
            [2]
        );
        assert_eq!(stats.forbidden_applied, 1);
        assert_eq!(stats.uturn_deadend, 3);

        // 나→다 뒤 다→나 U턴 허용
        let uturn = [TurnRule { st: 3, ed: 4 }];
        let (line_graph, stats) = LineGraph::build(&demo.input(&group, &internal, &[], &uturn));
        assert_eq!(
            line_graph.head[line_graph.first_out[3] as usize..line_graph.first_out[4] as usize],
            [4, 5]
        );
        assert_eq!(stats.uturn_allowed, 1);
    }

    #[test]
    fn expand_intersection_group() {
        // 모퉁이 A(1) B(2) C(3) D(4)가 내부 링크 A→B→C→D→A로 이어진 교차로. 서쪽 X(10)에서 A로 들어와 A에서 우회전 R(50), B에서 직진 S(60), C에서 좌회전 L(70), D에서 X 맞은편 X'(11)로 U턴한다
        let at = |id: u64, x: f64, y: f64| NodeInput::new(id, ll(x, y).0, ll(x, y).1);
        let nodes = [
            at(10, -100.0, -6.0),
            at(11, -100.0, 6.0),
            at(1, -6.0, -6.0),
            at(2, 6.0, -6.0),
            at(3, 6.0, 6.0),
            at(4, -6.0, 6.0),
            at(50, -6.0, -100.0),
            at(60, 100.0, -6.0),
            at(70, 6.0, 100.0),
        ];
        let links = vec![
            LinkInput::new(1000, 10, 1, 94.0), // X→A 진입
            LinkInput::new(1100, 4, 11, 94.0), // D→X'
            LinkInput::new(2001, 1, 2, 12.0),  // A→B
            LinkInput::new(2002, 2, 3, 12.0),  // B→C
            LinkInput::new(2003, 3, 4, 12.0),  // C→D
            LinkInput::new(2004, 4, 1, 12.0),  // D→A
            LinkInput::new(3000, 1, 50, 94.0), // A→R
            LinkInput::new(4000, 2, 60, 94.0), // B→S
            LinkInput::new(5000, 3, 70, 94.0), // C→L
        ];
        let demo = Demo::new(&nodes, links);
        let g = &demo.g;
        let crossing: Vec<bool> = g.node_id.iter().map(|&id| (1..=4).contains(&id)).collect();
        let group = intersection_groups(&g.first_out, &g.head, &g.length, &crossing, 25.0, &[]);
        let node = |id: u64| g.node_id.iter().position(|&x| x == id).unwrap();
        assert_eq!(group[node(1)], group[node(2)]);
        assert_eq!(group[node(1)], group[node(4)]);
        assert_eq!(group[node(10)], u32::MAX);
        let internal = internal_edges(&g.first_out, &g.head, &group);
        assert_eq!(internal.iter().filter(|&&b| b).count(), 4);

        let (line_graph, stats) = LineGraph::build(&demo.input(&group, &internal, &[], &[]));
        // 우회전, 직진, 좌회전 통행이 생기고 U턴은 없다
        assert_eq!(
            demo.outs(&line_graph, 1000),
            vec![
                (3000, 94.0, vec![]),
                (4000, 106.0, vec![2001]),
                (5000, 118.0, vec![2001, 2002])
            ]
        );
        assert_eq!(stats.expanded, 2);
        assert_eq!(stats.uturn_removed, 1);
        // 내부 링크에서 출발하는 통행은 없다
        let e = demo.edge(2001) as usize;
        assert_eq!(line_graph.first_out[e], line_graph.first_out[e + 1]);

        // 좌회전 금지와 U턴 허용
        let forbidden = [TurnRule {
            st: demo.edge(1000),
            ed: demo.edge(5000),
        }];
        let uturn = [TurnRule {
            st: demo.edge(1000),
            ed: demo.edge(1100),
        }];
        let (line_graph, stats) =
            LineGraph::build(&demo.input(&group, &internal, &forbidden, &uturn));
        assert_eq!(
            demo.outs(&line_graph, 1000),
            vec![
                (3000, 94.0, vec![]),
                (4000, 106.0, vec![2001]),
                (1100, 130.0, vec![2001, 2002, 2003])
            ]
        );
        assert_eq!(stats.forbidden_applied, 1);
        assert_eq!(stats.uturn_allowed, 1);
        assert_eq!(stats.expanded, 2);

        // 내부 링크 B→C를 st로 둔 금지도 적용된다
        let forbidden = [TurnRule {
            st: demo.edge(2002),
            ed: demo.edge(5000),
        }];
        let (line_graph, _) = LineGraph::build(&demo.input(&group, &internal, &forbidden, &[]));
        assert_eq!(
            demo.outs(&line_graph, 1000)
                .iter()
                .map(|o| o.0)
                .collect::<Vec<_>>(),
            vec![3000, 4000]
        );
    }

    #[test]
    fn join_rule_path_over_long_link() {
        // 신림로처럼 P→Q 진입, Q→R 연결 링크 30m, R→S 반대 방향 진출. Q에서 직진 Q→T도 있다. 다른 길 X→R도 R로 들어온다
        let at = |id: u64, x: f64, y: f64| NodeInput::new(id, ll(x, y).0, ll(x, y).1);
        let nodes = [
            at(1, -100.0, 0.0),
            at(2, 0.0, 0.0),
            at(3, 0.0, 30.0),
            at(4, -100.0, 30.0),
            at(5, 100.0, 0.0),
            at(6, 0.0, 130.0),
        ];
        let links = vec![
            LinkInput::new(100, 1, 2, 100.0), // P→Q 진입
            LinkInput::new(200, 2, 3, 30.0),  // Q→R 연결 링크
            LinkInput::new(300, 3, 4, 100.0), // R→S 진출
            LinkInput::new(400, 2, 5, 100.0), // Q→T 직진
            LinkInput::new(500, 6, 3, 100.0), // X→R
        ];
        let demo = Demo::new(&nodes, links);
        let g = &demo.g;
        let tail = tails(&g.first_out);
        let crossing = vec![true; 6];
        // 연결 링크 → 진출은 좌회전금지, 진입 → 진출은 허용 U턴
        let forbidden = [TurnRule {
            st: demo.edge(200),
            ed: demo.edge(300),
        }];
        let uturn = [TurnRule {
            st: demo.edge(100),
            ed: demo.edge(300),
        }];

        // 25m로만 묶으면 진입 → 진출 통행이 없어 허용 U턴이 걸리지 않는다
        let group = intersection_groups(&g.first_out, &g.head, &g.length, &crossing, 25.0, &[]);
        let internal = internal_edges(&g.first_out, &g.head, &group);
        let (line_graph, stats) =
            LineGraph::build(&demo.input(&group, &internal, &forbidden, &uturn));
        assert_eq!(demo.outs(&line_graph, 200), vec![]);
        assert_eq!(stats.uturn_rules_hit, 0);

        // 허용 U턴의 경로 Q, R을 한 그룹으로 묶으면 진입 → 연결 링크 → 진출 통행이 생기고 허용 U턴이 금지보다 우선한다
        let path = rule_path(
            &g.first_out,
            &g.head,
            &g.length,
            &tail,
            demo.edge(100),
            demo.edge(300),
            5,
        )
        .unwrap();
        let node = |id: u64| g.node_id.iter().position(|&x| x == id).unwrap() as u32;
        assert_eq!(path, vec![node(2), node(3)]);
        let group = intersection_groups(&g.first_out, &g.head, &g.length, &crossing, 25.0, &[path]);
        let internal = internal_edges(&g.first_out, &g.head, &group);
        let (line_graph, stats) =
            LineGraph::build(&demo.input(&group, &internal, &forbidden, &uturn));
        assert_eq!(
            demo.outs(&line_graph, 100),
            vec![(400, 100.0, vec![]), (300, 130.0, vec![200])]
        );
        assert_eq!(stats.uturn_rules_hit, 1);
        assert_eq!(stats.forbidden_overridden, 1);
        // X에서 R로 들어온 차는 진출 링크로 갈 수 있다. 금지는 연결 링크에서 온 경우만이다
        assert_eq!(demo.outs(&line_graph, 500), vec![(300, 100.0, vec![])]);
    }

    #[test]
    fn bridge_uturn_without_link() {
        // IC처럼 진출 램프 P→Q가 Q에서 끝나고 반대 방향 진입 램프 R→S가 Q에서 8m 떨어진 R에서 시작한다. Q와 R을 잇는 링크는 없다
        let at = |id: u64, x: f64, y: f64| NodeInput::new(id, ll(x, y).0, ll(x, y).1);
        let nodes = [
            at(1, -100.0, 0.0),
            at(2, 0.0, 0.0),
            at(3, 0.0, 8.0),
            at(4, -100.0, 8.0),
            at(5, 0.0, -100.0),
            at(6, 0.0, 108.0),
        ];
        let links = vec![
            LinkInput::new(100, 1, 2, 100.0), // P→Q 진출 램프
            LinkInput::new(200, 3, 4, 100.0), // R→S 진입 램프
            LinkInput::new(300, 2, 5, 100.0), // Q에서 일반도로
            LinkInput::new(400, 6, 3, 100.0), // 일반도로에서 R
        ];
        let demo = Demo::new(&nodes, links);
        let g = &demo.g;
        let tail = tails(&g.first_out);
        assert_eq!(
            rule_path(
                &g.first_out,
                &g.head,
                &g.length,
                &tail,
                demo.edge(100),
                demo.edge(200),
                5
            ),
            None
        );
        let group = vec![u32::MAX; 6];
        let internal = vec![false; 4];
        let uturn = [TurnRule {
            st: demo.edge(100),
            ed: demo.edge(200),
        }];
        let mut input = demo.input(&group, &internal, &[], &uturn);
        let (line_graph, _) = LineGraph::build(&input);
        assert_eq!(demo.outs(&line_graph, 100), vec![(300, 100.0, vec![])]);

        // 건너가는 통행의 비용은 Q와 R 사이 8m와 진입 램프 길이의 합
        input.bridges = &uturn;
        let (line_graph, stats) = LineGraph::build(&input);
        let outs = demo.outs(&line_graph, 100);
        assert_eq!(outs.len(), 2);
        assert_eq!(outs[1].0, 200);
        assert!((outs[1].1 - 108.0).abs() < 1e-6);
        assert_eq!(stats.bridged, 1);
        assert_eq!(stats.uturn_rules_hit, 1);
    }
}
