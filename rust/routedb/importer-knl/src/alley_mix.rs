//! 골목을 큰길 그래프에 섞는 실험. 환경 변수 `ROUTE_ALLEY_MIX`(maps/골목 아래 폴더 이름을 쉼표로 구분)가 있을 때만 동작한다.
//! 골목이 표준노드링크 링크 중간에 닿는 자리에서 그 링크를 나누고 골목 노드와 양방향 골목 링크를 더한다.
//! 골목 파일(alley.rdb) 방식과 크기·시간을 비교하기 위한 것이며 기본 빌드에는 쓰지 않는다

use std::collections::HashMap;
use std::path::Path;

use routedb_core::graph::{LinkInput, NodeInput};

use crate::alley::{self, AlleyLink, AlleyNode};

/// 골목 링크의 속도(km/h). 골목 파일 방식의 골목 소요시간 속도와 같다
const ALLEY_MAX_SPD: u16 = 10;
/// 골목 링크의 도로등급. 108 기타
const ALLEY_ROAD_RANK: u8 = 108;
/// 나뉜 표준노드링크 뒷부분의 새 LINK_ID 시작
const SPLIT_LINK_BASE: u64 = 9_500_000_000;

/// 나눈 링크의 조각 목록. 원래 LINK_ID마다 (조각 LINK_ID, 시작 비율, 끝 비율)
#[derive(Default)]
pub struct Splits {
    pieces: HashMap<u64, Vec<(u64, f64, f64)>>,
}

impl Splits {
    /// 나눈 원래 링크 수
    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    /// 링크 끝 쪽 조각. 회전정보의 진입 링크는 끝이 교차로에 닿으므로 이 조각으로 바꾼다
    pub fn last(&self, id: u64) -> u64 {
        self.pieces
            .get(&id)
            .and_then(|p| p.last())
            .map_or(id, |p| p.0)
    }

    /// 링크 위 비율 위치(× 10,000)를 그 위치가 든 조각과 조각 안 비율로 바꾼다
    pub fn locate(&self, id: u64, at: u16) -> (u64, u16) {
        let Some(list) = self.pieces.get(&id) else {
            return (id, at);
        };
        let r = at as f64 / 10_000.0;
        let &(piece, from, to) = list
            .iter()
            .find(|p| r < p.2)
            .unwrap_or_else(|| list.last().unwrap());
        let local = ((r - from) / (to - from)).clamp(0.0, 1.0);
        (piece, (local * 10_000.0).round() as u16)
    }
}

/// 평면 근사 거리(m)
fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dy = (b.0 - a.0) * 111_320.0;
    let dx = (b.1 - a.1) * 111_320.0 * 0.809_017;
    (dx * dx + dy * dy).sqrt()
}

/// (위도, 경도) 좌표 열
type Points = Vec<(f64, f64)>;

/// 좌표 열을 길이 비율 at에서 둘로 나눈다. 나누는 점은 양쪽에 모두 들어간다
fn split_geometry(points: &[(f64, f64)], at: f64) -> (Points, Points) {
    let total: f64 = points.windows(2).map(|w| dist(w[0], w[1])).sum();
    let target = total * at;
    let mut walked = 0.0;
    for i in 0..points.len() - 1 {
        let seg = dist(points[i], points[i + 1]);
        if walked + seg >= target && seg > 0.0 {
            let t = ((target - walked) / seg).clamp(0.0, 1.0);
            let p = (
                points[i].0 + (points[i + 1].0 - points[i].0) * t,
                points[i].1 + (points[i + 1].1 - points[i].1) * t,
            );
            let mut head = points[..=i].to_vec();
            head.push(p);
            let mut tail = vec![p];
            tail.extend_from_slice(&points[i + 1..]);
            return (head, tail);
        }
        walked += seg;
    }
    let last = points[points.len() - 1];
    (points.to_vec(), vec![last, last])
}

/// 교차로가 놓인 표준노드링크 링크를 위치 순서로 나눈다. 앞부분은 원래 LINK_ID를 유지하고 뒷부분은 새 LINK_ID를 받는다
pub fn split_links(links: &mut Vec<LinkInput>, nodes: &[AlleyNode]) -> Splits {
    let mut cuts: HashMap<u64, Vec<(f64, u64)>> = HashMap::new();
    for n in nodes {
        let Some(at) = n.at else { continue };
        if let Some(l) = n.on_link {
            cuts.entry(l).or_default().push((at, n.id));
        }
        if let Some(l) = n.on_link_rev {
            cuts.entry(l).or_default().push((1.0 - at, n.id));
        }
    }
    let mut next_id = SPLIT_LINK_BASE;
    let mut added = Vec::new();
    let mut splits = Splits::default();
    for link in links.iter_mut() {
        let Some(list) = cuts.get_mut(&link.id) else {
            continue;
        };
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        let original_len = link.length;
        let mut rest_from = 0.0;
        let mut current = link.clone();
        let mut pieces: Vec<LinkInput> = Vec::new();
        let mut ranges = Vec::new();
        for &(at, node) in list.iter() {
            if at <= rest_from + 1e-6 || at >= 1.0 - 1e-6 {
                continue;
            }
            let local = (at - rest_from) / (1.0 - rest_from);
            let (head, tail) = split_geometry(&current.geometry, local);
            let mut first = current.clone();
            first.geometry = head;
            first.length = original_len * (at - rest_from);
            first.to = node;
            ranges.push((first.id, rest_from, at));
            pieces.push(first);
            current.geometry = tail;
            current.from = node;
            current.id = next_id;
            next_id += 1;
            rest_from = at;
        }
        if pieces.is_empty() {
            continue;
        }
        current.length = original_len * (1.0 - rest_from);
        ranges.push((current.id, rest_from, 1.0));
        splits.pieces.insert(link.id, ranges);
        let mut iter = pieces.into_iter();
        *link = iter.next().unwrap();
        added.extend(iter);
        added.push(current);
    }
    links.extend(added);
    splits
}

/// 골목 노드와 양방향 골목 링크(정방향 LINK_ID, 역방향 LINK_ID + 1)
pub fn inputs(nodes: &[AlleyNode], links: &[AlleyLink]) -> (Vec<NodeInput>, Vec<LinkInput>) {
    let node_inputs = nodes
        .iter()
        .map(|n| NodeInput::new(n.id, n.lat, n.lon))
        .collect();
    let mut link_inputs = Vec::with_capacity(links.len() * 2);
    for l in links {
        for (id, from, to, forward) in [(l.id, l.from, l.to, true), (l.id + 1, l.to, l.from, false)]
        {
            let mut input = LinkInput::new(id, from, to, l.length);
            input.geometry = if forward {
                l.geometry.clone()
            } else {
                l.geometry.iter().rev().copied().collect()
            };
            input.max_spd = ALLEY_MAX_SPD;
            input.road_rank = ALLEY_ROAD_RANK;
            input.road_name = l.name.clone();
            link_inputs.push(input);
        }
    }
    (node_inputs, link_inputs)
}

/// `ROUTE_ALLEY_MIX`의 폴더들에서 골목을 읽는다. 환경 변수가 없으면 빈 목록이다
pub fn read(alley_dir: &Path) -> (Vec<AlleyNode>, Vec<AlleyLink>) {
    let Some(list) = std::env::var_os("ROUTE_ALLEY_MIX") else {
        return (Vec::new(), Vec::new());
    };
    let (mut nodes, mut links) = (Vec::new(), Vec::new());
    for name in list.to_string_lossy().split(',').filter(|s| !s.is_empty()) {
        let (n, l) = alley::read_all(&alley_dir.join(name.trim()));
        nodes.extend(n);
        links.extend(l);
    }
    (nodes, links)
}
