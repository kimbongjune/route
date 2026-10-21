// 그래프 파일의 line graph로 CCH를 만들고 build, customize, query 시간을 재며 다익스트라와 결과를 비교한다.
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use cch::path::PathQuery;
use cch::{degree_order, distance, inertial_order, node_path, Cch, INF_WEIGHT};
use routedb_core::dijkstra;
use routedb_core::file::GraphFile;
use routedb_core::hierarchy::{cch_graph, edges_of_node_path, tails};

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";
const PAIRS: usize = 300;

fn main() {
    let use_degree = std::env::args().any(|a| a == "--degree");
    let file = GraphFile::open(Path::new(GRAPH_FILE_LOCATION)).unwrap();
    let (first_out, head, weight) = (file.turn_first_out(), file.turn_head(), file.turn_weight());
    let input = cch_graph(first_out, head, weight);
    // 다익스트라도 CCH와 같은 cm 단위 정수 가중치로 비교한다
    let weight_cm: Vec<f64> = input.weight.iter().map(|&w| w as f64).collect();
    println!("링크 {} 통행 {}", input.node_count(), input.arc_count());

    let started = Instant::now();
    let order = if use_degree {
        degree_order(&input)
    } else {
        // 순서 계산 좌표는 링크 좌표의 가운데 점이다
        let (geom_first, geom_lat, geom_lon) = (file.geom_first(), file.geom_lat(), file.geom_lon());
        let mid = |e: usize| (geom_first[e] as usize + geom_first[e + 1] as usize) / 2;
        let lat: Vec<f32> = (0..file.edge_count()).map(|e| geom_lat[mid(e)] as f32).collect();
        let lon: Vec<f32> = (0..file.edge_count()).map(|e| geom_lon[mid(e)] as f32).collect();
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
    // 경로 CLI가 스냅한 출발 링크와 도착 링크
    let pos: HashMap<u64, u32> = file.link_id().iter().enumerate().map(|(i, &id)| (id, i as u32)).collect();
    for (name, from_id, to_id) in [("서울역 -> 강남역사거리", 1020021819, 1220021003), ("서울역 -> 부산역", 1020021819, 1320003802)] {
        let (from, to) = (pos[&from_id], pos[&to_id]);
        let started = Instant::now();
        let d = distance(&cch_v, &metric_v, from, to);
        let t_dist = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let path = node_path(&cch_v, &metric_v, from, to).unwrap();
        let t_path = started.elapsed().as_secs_f64() * 1000.0;
        let (dd, dij_turns, _) = dijkstra::shortest_path(first_out, head, &weight_cm, from, to).unwrap();
        let turns = edges_of_node_path(first_out, head, &weight_cm, &path);
        let links = 1 + turns.iter().map(|&t| file.via_of(t).len() + 1).sum::<usize>();
        println!("{} CCH {:.2}m 거리 {:.2}ms 경로 {:.2}ms 통행 {}개 링크 {}개 | 다익스트라 {:.2}m | 통행 열 같음 {}",
                 name, d as f64 / 100.0, t_dist, t_path, turns.len(), links, dd / 100.0, turns == dij_turns);
    }

    // 고정 시드 LCG로 무작위 쌍을 만들어 다익스트라와 대조
    let n = file.edge_count() as u64;
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
    let dij_d: Vec<u32> = pairs.iter().map(|&(a, b)| dijkstra::shortest_path(first_out, head, &weight_cm, a, b).map_or(INF_WEIGHT, |r| r.0.round() as u32)).collect();
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
