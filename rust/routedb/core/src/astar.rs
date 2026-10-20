use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// 힙에 넣는 (예상 총 거리, 교차로) BinaryHeap은 큰 값부터 꺼내므로 반대로 비교해 예상 총 거리가 짧은 교차로부터 꺼낸다.
#[derive(PartialEq)]
struct State {
    estimate: f64,
    node: u32,
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other.estimate.total_cmp(&self.estimate)
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 두 위경도 사이의 대원 거리(m)를 계산한다.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let (lat1, lat2) = (lat1.to_radians(), lat2.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().asin()
}

/// from 교차로에서 to 교차로까지 A*로 최단 경로를 탐색한다. 휴리스틱은 도착 교차로까지의 대원 거리이다.
/// (총 길이, 경로의 간선 번호 목록, 방문한 교차로 수)를 반환하고 갈 수 없으면 None을 반환한다.
pub fn shortest_path(
    first_out: &[u32], head: &[u32], length: &[f64], lat: &[f64], lon: &[f64], from: u32, to: u32,
) -> Option<(f64, Vec<usize>, usize)> {
    let n = first_out.len() - 1;
    let (to_lat, to_lon) = (lat[to as usize], lon[to as usize]);
    let h = |v: u32| haversine_m(lat[v as usize], lon[v as usize], to_lat, to_lon);

    let mut dist = vec![f64::INFINITY; n];
    let mut came_by = vec![usize::MAX; n];
    let mut heap = BinaryHeap::new();
    let mut settled = 0;

    dist[from as usize] = 0.0;
    heap.push(State { estimate: h(from), node: from });

    while let Some(State { estimate, node: v }) = heap.pop() {
        let d = dist[v as usize];
        if v == to {
            break;
        }
        // 더 짧은 거리로 이미 처리한 교차로는 건너뛴다
        if estimate > d + h(v) {
            continue;
        }
        settled += 1;
        for e in first_out[v as usize] as usize..first_out[v as usize + 1] as usize {
            let w = head[e];
            let nd = d + length[e];
            if nd < dist[w as usize] {
                dist[w as usize] = nd;
                came_by[w as usize] = e;
                heap.push(State { estimate: nd + h(w), node: w });
            }
        }
    }

    if dist[to as usize].is_infinite() {
        return None;
    }

    let mut edges = Vec::new();
    let mut v = to;
    while v != from {
        let e = came_by[v as usize];
        edges.push(e);
        v = (first_out.partition_point(|&f| f as usize <= e) - 1) as u32;
    }
    edges.reverse();
    Some((dist[to as usize], edges, settled))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;

    fn demo() -> Graph {
        let nodes = [
            (1000000101, 37.000, 127.000),
            (1000000201, 37.000, 127.001),
            (1000000301, 37.001, 127.001),
            (1000000401, 37.001, 127.000),
            (1000000501, 37.002, 127.001),
        ];
        let links = [
            (1000001000, 1000000101, 1000000201, 100.0),
            (1000001100, 1000000201, 1000000101, 100.0),
            (1000002000, 1000000201, 1000000301, 120.0),
            (1000002100, 1000000301, 1000000201, 120.0),
            (1000003000, 1000000101, 1000000401, 120.0),
            (1000004000, 1000000401, 1000000301, 90.0),
            (1000005000, 1000000301, 1000000501, 120.0),
        ];
        Graph::build(&nodes, &links).0
    }

    #[test]
    fn haversine_seoul_busan() {
        // 서울역 37.5547, 126.9707 / 부산역 35.1152, 129.0422 측지 거리 328,409m
        let d = haversine_m(37.5547, 126.9707, 35.1152, 129.0422);
        assert!((d - 328_409.0).abs() < 500.0, "{d}");
    }

    #[test]
    fn same_result_as_dijkstra() {
        let g = demo();
        let (d1, e1, _) = crate::dijkstra::shortest_path(&g.first_out, &g.head, &g.length, 0, 4).unwrap();
        let (d2, e2, _) = shortest_path(&g.first_out, &g.head, &g.length, &g.lat, &g.lon, 0, 4).unwrap();
        assert_eq!(d1, d2);
        assert_eq!(e1, e2);
    }

    #[test]
    fn unreachable() {
        let g = demo();
        assert!(shortest_path(&g.first_out, &g.head, &g.length, &g.lat, &g.lon, 4, 0).is_none());
    }
}
