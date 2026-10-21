// 그래프 파일로 CCH를 만들고 build, customize, query 시간을 재며 다익스트라와 결과를 비교한다.
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use cch::path::PathQuery;
use cch::{degree_order, distance, inertial_order, node_path, Cch, INF_WEIGHT};
use routedb_core::dijkstra;
use routedb_core::file::GraphFile;
use routedb_core::hierarchy::{cch_graph, tails};

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";
const PAIRS: usize = 300;

fn main() {
    let use_degree = std::env::args().any(|a| a == "--degree");
    let file = GraphFile::open(Path::new(GRAPH_FILE_LOCATION)).unwrap();
    let (first_out, head, length) = (file.first_out(), file.head(), file.length());
    let input = cch_graph(first_out, head, length);
    // 다익스트라도 CCH와 같은 cm 단위 정수 가중치로 비교한다
    let length_cm: Vec<f64> = input.weight.iter().map(|&w| w as f64).collect();
    println!("노드 {} 간선 {}", input.node_count(), input.arc_count());

    let started = Instant::now();
    let order = if use_degree {
        degree_order(&input)
    } else {
        let lat: Vec<f32> = file.lat().iter().map(|&v| v as f32).collect();
        let lon: Vec<f32> = file.lon().iter().map(|&v| v as f32).collect();
        inertial_order(input.node_count() as u32, &tails(first_out), head, &lat, &lon)
    };
    println!("순서 계산 {} {:.1}초", if use_degree { "degree_order" } else { "inertial_order" }, started.elapsed().as_secs_f64());

    let started = Instant::now();
    let cch = Cch::build(&input, &order);
    println!("build CCH 간선 {} {:.1}초", cch.cch_arc_count(), started.elapsed().as_secs_f64());

    let started = Instant::now();
    let metric = cch.customize(&input.weight);
    println!("customize {:.0}ms", started.elapsed().as_secs_f64() * 1000.0);

    let (cch_v, metric_v) = (cch.view(), metric.view());
    let pos: HashMap<u64, u32> = file.node_id().iter().enumerate().map(|(i, &id)| (id, i as u32)).collect();
    for (name, from_id, to_id) in [("서울역 -> 강남역사거리", 1010009300, 1210034101), ("서울역 -> 부산역", 1010009300, 1320000204)] {
        let (from, to) = (pos[&from_id], pos[&to_id]);
        let started = Instant::now();
        let d = distance(&cch_v, &metric_v, from, to);
        let t_dist = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let path = node_path(&cch_v, &metric_v, from, to).unwrap();
        let t_path = started.elapsed().as_secs_f64() * 1000.0;
        let (dd, _, _) = dijkstra::shortest_path(first_out, head, &length_cm, from, to).unwrap();
        println!("{} CCH {:.2}m 거리 {:.2}ms 경로 {:.2}ms 교차로 {}개 | 다익스트라 {:.2}m", name, d as f64 / 100.0, t_dist, t_path, path.len(), dd / 100.0);
    }

    // 고정 시드 LCG로 무작위 쌍을 만들어 다익스트라와 대조
    let n = file.node_count() as u64;
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) % n) as u32
    };
    let pairs: Vec<(u32, u32)> = (0..PAIRS).map(|_| (next(), next())).collect();
    let started = Instant::now();
    let cch_d: Vec<u32> = pairs.iter().map(|&(a, b)| distance(&cch_v, &metric_v, a, b)).collect();
    let t_cch = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let dij_d: Vec<u32> = pairs.iter().map(|&(a, b)| dijkstra::shortest_path(first_out, head, &length_cm, a, b).map_or(INF_WEIGHT, |r| r.0.round() as u32)).collect();
    let t_dij = started.elapsed().as_secs_f64() * 1000.0;
    let same = cch_d.iter().zip(&dij_d).filter(|(a, b)| a == b).count();
    println!("무작위 {}쌍 CCH distance 합계 {:.1}ms 평균 {:.3}ms | 다익스트라 합계 {:.0}ms 평균 {:.1}ms | 거리 같음 {}/{}",
             PAIRS, t_cch, t_cch / PAIRS as f64, t_dij, t_dij / PAIRS as f64, same, PAIRS);

    // PathQuery를 재사용하면 버퍼 할당이 한 번뿐이다
    let mut pq = PathQuery::new(&cch_v);
    let started = Instant::now();
    let found = pairs.iter().filter(|&&(a, b)| pq.path(&metric_v, a, b).is_some()).count();
    let t_pq = started.elapsed().as_secs_f64() * 1000.0;
    println!("무작위 {}쌍 PathQuery 재사용 경로 있음 {} 합계 {:.1}ms 평균 {:.3}ms", PAIRS, found, t_pq, t_pq / PAIRS as f64);
}
