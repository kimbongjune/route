use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// 힙에 넣는 (거리, 교차로) BinaryHeap은 큰 값부터 꺼내므로 거리를 반대로 비교해 짧은 거리부터 꺼낸다.
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

/// from 교차로에서 to 교차로까지 최단 경로를 찾는다.
/// (총 길이, 지나간 간선 번호 목록, 방문한 교차로 수)를 반환하고 갈 수 없으면 None을 반환한다.
pub fn shortest_path(
    first_out: &[u32],
    head: &[u32],
    length: &[f64],
    from: u32,
    to: u32,
) -> Option<(f64, Vec<usize>, usize)> {
    let n = first_out.len() - 1;
    // 교차로마다 지금까지 찾은 최단 거리
    let mut dist = vec![f64::INFINITY; n];
    // 교차로마다 최단 거리로 진입한 간선 번호
    let mut came_by = vec![usize::MAX; n];
    let mut heap = BinaryHeap::new();
    let mut settled = 0;

    dist[from as usize] = 0.0;
    heap.push(State {
        dist: 0.0,
        node: from,
    });

    while let Some(State { dist: d, node: v }) = heap.pop() {
        if v == to {
            break;
        }
        // 이미 더 짧은 거리로 처리한 교차로는 건너뛴다
        if d > dist[v as usize] {
            continue;
        }
        settled += 1;
        for e in first_out[v as usize] as usize..first_out[v as usize + 1] as usize {
            let w = head[e];
            let nd = d + length[e];
            if nd < dist[w as usize] {
                dist[w as usize] = nd;
                came_by[w as usize] = e;
                heap.push(State { dist: nd, node: w });
            }
        }
    }

    if dist[to as usize].is_infinite() {
        return None;
    }

    // 도착 교차로부터 진입 간선을 역방향으로 추적하여 경로를 생성한다
    let mut edges = Vec::new();
    let mut v = to;
    while v != from {
        let e = came_by[v as usize];
        edges.push(e);
        v = tail_of(first_out, e);
    }
    edges.reverse();
    Some((dist[to as usize], edges, settled))
}

/// first_out에서 간선 번호 e의 출발 교차로를 탐색한다.
fn tail_of(first_out: &[u32], e: usize) -> u32 {
    (first_out.partition_point(|&f| f as usize <= e) - 1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Graph, LinkInput, NodeInput};

    fn demo() -> Graph {
        let nodes = [
            NodeInput::new(1000000101, 37.000, 127.000),
            NodeInput::new(1000000201, 37.000, 127.001),
            NodeInput::new(1000000301, 37.001, 127.001),
            NodeInput::new(1000000401, 37.001, 127.000),
            NodeInput::new(1000000501, 37.002, 127.001),
        ];
        let links = [
            LinkInput::new(1000001000, 1000000101, 1000000201, 100.0), // 가 -> 나
            LinkInput::new(1000001100, 1000000201, 1000000101, 100.0), // 나 -> 가
            LinkInput::new(1000002000, 1000000201, 1000000301, 100.0), // 나 -> 다
            LinkInput::new(1000002100, 1000000301, 1000000201, 100.0), // 다 -> 나
            LinkInput::new(1000003000, 1000000101, 1000000401, 50.0),  // 가 -> 라
            LinkInput::new(1000004000, 1000000401, 1000000301, 80.0),  // 라 -> 다
            LinkInput::new(1000005000, 1000000301, 1000000501, 100.0), // 다 -> 마
        ];
        Graph::build(&nodes, &links).0
    }

    #[test]
    fn shortest_from_ga_to_ma() {
        let g = demo();
        let (dist, edges, _) = shortest_path(&g.first_out, &g.head, &g.length, 0, 4).unwrap();
        let links: Vec<u64> = edges.iter().map(|&e| g.link_id[e]).collect();
        assert_eq!(dist, 230.0);
        assert_eq!(links, vec![1000003000, 1000004000, 1000005000]);
    }

    #[test]
    fn unreachable() {
        // 마에서 출발하는 간선 없음
        let g = demo();
        assert!(shortest_path(&g.first_out, &g.head, &g.length, 4, 0).is_none());
    }

    #[test]
    fn same_node() {
        let g = demo();
        assert_eq!(
            shortest_path(&g.first_out, &g.head, &g.length, 2, 2),
            Some((0.0, vec![], 0))
        );
    }
}
