// shp 순서와 힐베르트 순서로 번호를 부여한 그래프에서 무작위 300쌍의 다익스트라 소요 시간을 비교한다.
#[path = "../src/coord.rs"]
mod coord;
#[path = "../src/moct.rs"]
mod moct;

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use routedb_core::graph::Graph;
use routedb_core::{dijkstra, hilbert};

use coord::TmToWgs84;
use moct::MOCT_SHPFILE_FOLDER_LOCATION;

const PAIRS: usize = 300;

fn main() {
    let folder = Path::new(MOCT_SHPFILE_FOLDER_LOCATION);
    let nodes = moct::read_nodes(folder);
    let links = moct::read_links(folder);
    let tm = TmToWgs84::new();
    let to_latlon = |&(id, x, y): &(u64, f64, f64)| {
        let (lat, lon) = tm.convert(x, y);
        (id, lat, lon)
    };

    let shp_order: Vec<(u64, f64, f64)> = nodes.iter().map(to_latlon).collect();
    let points: Vec<(f64, f64)> = nodes.iter().map(|n| (n.1, n.2)).collect();
    let hilbert_order: Vec<(u64, f64, f64)> = hilbert::sort_order(&points).iter().map(|&i| to_latlon(&nodes[i])).collect();

    let (g_shp, _) = Graph::build(&shp_order, &links);
    let (g_hil, _) = Graph::build(&hilbert_order, &links);

    // 고정 시드 LCG로 같은 쌍을 두 그래프에 사용한다
    let n = g_shp.node_count() as u64;
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let pairs: Vec<(u64, u64)> = (0..PAIRS).map(|_| (g_shp.node_id[next() as usize], g_shp.node_id[next() as usize])).collect();

    for (name, g) in [("shp 순서", &g_shp), ("힐베르트 순서", &g_hil)] {
        let pos: HashMap<u64, u32> = g.node_id.iter().enumerate().map(|(i, &id)| (id, i as u32)).collect();
        let started = Instant::now();
        let found = pairs.iter().filter(|&&(a, b)| dijkstra::shortest_path(&g.first_out, &g.head, &g.length, pos[&a], pos[&b]).is_some()).count();
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        println!("{} {}쌍 중 경로 있음 {} 합계 {:.0}ms 평균 {:.1}ms", name, PAIRS, found, ms, ms / PAIRS as f64);
    }
}
