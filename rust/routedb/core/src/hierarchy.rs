/// 간선 길이(m)를 CCH 가중치(cm, u32)로 변환한다.
pub fn weights_cm(length: &[f64]) -> Vec<u32> {
    length.iter().map(|&m| (m * 100.0).round() as u32).collect()
}

/// first_out, head로 cch 입력 그래프를 만든다.
pub fn cch_graph(first_out: &[u32], head: &[u32], length: &[f64]) -> cch::graph::Graph {
    cch::graph::Graph { first_out: first_out.to_vec(), head: head.to_vec(), weight: weights_cm(length) }
}

/// 간선마다 출발 교차로 번호를 만든다. inertial_order 입력용이다.
pub fn tails(first_out: &[u32]) -> Vec<u32> {
    let mut tail = Vec::with_capacity(*first_out.last().unwrap_or(&0) as usize);
    for v in 0..first_out.len() - 1 {
        for _ in first_out[v]..first_out[v + 1] {
            tail.push(v as u32);
        }
    }
    tail
}

/// inertial_order로 순서를 계산하고 CCH 구조와 길이 가중치 metric을 만든다.
pub fn build(first_out: &[u32], head: &[u32], length: &[f64], lat: &[f64], lon: &[f64]) -> (cch::Cch, cch::Metric) {
    let input = cch_graph(first_out, head, length);
    let lat: Vec<f32> = lat.iter().map(|&v| v as f32).collect();
    let lon: Vec<f32> = lon.iter().map(|&v| v as f32).collect();
    let order = cch::inertial_order(input.node_count() as u32, &tails(first_out), head, &lat, &lon);
    let cch = cch::Cch::build(&input, &order);
    let metric = cch.customize(&input.weight);
    (cch, metric)
}

/// CCH가 반환한 교차로 번호 열을 간선 번호 열로 변환한다. 같은 두 교차로 사이에 간선이 여러 개면 가장 짧은 간선을 고른다.
pub fn edges_of_node_path(first_out: &[u32], head: &[u32], length: &[f64], nodes: &[u32]) -> Vec<usize> {
    nodes
        .windows(2)
        .map(|w| {
            (first_out[w[0] as usize] as usize..first_out[w[0] as usize + 1] as usize)
                .filter(|&e| head[e] == w[1])
                .min_by(|&a, &b| length[a].total_cmp(&length[b]))
                .expect("교차로 열이 간선으로 이어지지 않음")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dijkstra;
    use crate::graph::{Graph, LinkInput};
    use cch::{degree_order, distance, node_path, Cch, INF_WEIGHT};

    /// 가(0) 나(1) 다(2) 라(3) 마(4). 가↔나, 나↔다는 양방향이고 가→라 50, 라→다 80, 다→마 100은 일방통행이다.
    fn demo() -> Graph {
        let nodes = [
            (1000000101, 37.000, 127.000),
            (1000000201, 37.000, 127.001),
            (1000000301, 37.001, 127.001),
            (1000000401, 37.001, 127.000),
            (1000000501, 37.002, 127.001),
        ];
        let links = vec![
            LinkInput::new(1000001000, 1000000101, 1000000201, 100.0),
            LinkInput::new(1000001100, 1000000201, 1000000101, 100.0),
            LinkInput::new(1000002000, 1000000201, 1000000301, 100.0),
            LinkInput::new(1000002100, 1000000301, 1000000201, 100.0),
            LinkInput::new(1000003000, 1000000101, 1000000401, 50.0),
            LinkInput::new(1000004000, 1000000401, 1000000301, 80.0),
            LinkInput::new(1000005000, 1000000301, 1000000501, 100.0),
        ];
        Graph::build(&nodes, &links).0
    }

    #[test]
    fn same_as_dijkstra_with_one_way() {
        let g = demo();
        let input = cch_graph(&g.first_out, &g.head, &g.length);
        let order = degree_order(&input);
        let cch = Cch::build(&input, &order);
        let metric = cch.customize(&input.weight);
        let (cch_v, metric_v) = (cch.view(), metric.view());

        // 가 -> 마는 가→라→다→마 230m
        assert_eq!(distance(&cch_v, &metric_v, 0, 4), 23_000);
        assert_eq!(node_path(&cch_v, &metric_v, 0, 4), Some(vec![0, 3, 2, 4]));
        // 마에서 출발하는 간선이 없어 마 -> 가는 경로 없음
        assert_eq!(distance(&cch_v, &metric_v, 4, 0), INF_WEIGHT);
        // 다 -> 라는 일방통행 역방향이라 다→나→가→라 250m로 돌아간다
        assert_eq!(distance(&cch_v, &metric_v, 2, 3), 25_000);
        assert_eq!(node_path(&cch_v, &metric_v, 2, 3), Some(vec![2, 1, 0, 3]));

        // 모든 쌍의 거리와 간선 열을 다익스트라와 대조
        for s in 0..5u32 {
            for t in 0..5u32 {
                let expected = dijkstra::shortest_path(&g.first_out, &g.head, &g.length, s, t);
                let got = distance(&cch_v, &metric_v, s, t);
                assert_eq!(expected.as_ref().map_or(INF_WEIGHT, |r| (r.0 * 100.0).round() as u32), got, "{s} -> {t}");
                if let Some((_, edges, _)) = expected {
                    let nodes = node_path(&cch_v, &metric_v, s, t).unwrap();
                    assert_eq!(edges_of_node_path(&g.first_out, &g.head, &g.length, &nodes), edges, "{s} -> {t}");
                }
            }
        }
    }
}
