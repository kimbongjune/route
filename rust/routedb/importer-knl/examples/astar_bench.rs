// 그래프 파일을 열어 다익스트라와 A*의 결과와 소요 시간을 비교한다.
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use routedb_core::file::GraphFile;
use routedb_core::{astar, dijkstra};

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";
const PAIRS: usize = 300;

fn main() {
    let file = GraphFile::open(Path::new(GRAPH_FILE_LOCATION)).unwrap();
    let (first_out, head, length, lat, lon) = (
        file.first_out(),
        file.head(),
        file.length(),
        file.lat(),
        file.lon(),
    );
    let pos: HashMap<u64, u32> = file
        .node_id()
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i as u32))
        .collect();

    for (name, from_id, to_id) in [
        ("서울역 -> 강남역사거리", 1010009300, 1210034101),
        ("서울역 -> 부산역", 1010009300, 1320000204),
    ] {
        let (from, to) = (pos[&from_id], pos[&to_id]);
        let started = Instant::now();
        let (d1, e1, s1) = dijkstra::shortest_path(first_out, head, length, from, to).unwrap();
        let t1 = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let (d2, e2, s2) =
            astar::shortest_path(first_out, head, length, lat, lon, from, to).unwrap();
        let t2 = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{} 다익스트라 {:.0}m {:.0}ms 방문 교차로 {} | A* {:.0}m {:.0}ms 방문 교차로 {} | 경로 같음 {}",
            name,
            d1,
            t1,
            s1,
            d2,
            t2,
            s2,
            e1 == e2
        );
    }

    // 고정 시드 LCG로 같은 쌍을 두 알고리즘에 사용한다
    let n = file.node_count() as u64;
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) % n) as u32
    };
    let pairs: Vec<(u32, u32)> = (0..PAIRS).map(|_| (next(), next())).collect();

    let started = Instant::now();
    let r1: Vec<Option<(f64, usize)>> = pairs
        .iter()
        .map(|&(a, b)| dijkstra::shortest_path(first_out, head, length, a, b).map(|r| (r.0, r.2)))
        .collect();
    let t1 = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let r2: Vec<Option<(f64, usize)>> = pairs
        .iter()
        .map(|&(a, b)| {
            astar::shortest_path(first_out, head, length, lat, lon, a, b).map(|r| (r.0, r.2))
        })
        .collect();
    let t2 = started.elapsed().as_secs_f64() * 1000.0;
    let same = r1
        .iter()
        .zip(&r2)
        .filter(|(a, b)| a.map(|x| x.0) == b.map(|x| x.0))
        .count();
    let visited = |r: &[Option<(f64, usize)>]| {
        r.iter().flatten().map(|x| x.1).sum::<usize>() / r.iter().flatten().count()
    };
    println!(
        "무작위 {}쌍 다익스트라 합계 {:.0}ms 평균 {:.1}ms 방문 교차로 평균 {} | A* 합계 {:.0}ms 평균 {:.1}ms 방문 교차로 평균 {} | 거리 같음 {}/{}",
        PAIRS,
        t1,
        t1 / PAIRS as f64,
        visited(&r1),
        t2,
        t2 / PAIRS as f64,
        visited(&r2),
        same,
        PAIRS
    );
}
