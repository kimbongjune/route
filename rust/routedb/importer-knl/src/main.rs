mod coord;
mod moct;

use std::path::Path;
use std::time::Instant;

use routedb_core::file::GraphFile;
use routedb_core::graph::Graph;
use routedb_core::{dijkstra, hilbert};

use coord::TmToWgs84;
use moct::MOCT_SHPFILE_FOLDER_LOCATION;

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";

fn main() {
    let folder = Path::new(MOCT_SHPFILE_FOLDER_LOCATION);
    let nodes = moct::read_nodes(folder);
    let links = moct::read_links(folder);

    // 힐베르트 순서로 정렬하고 TM 좌표를 위경도로 변환
    let tm = TmToWgs84::new();
    let points: Vec<(f64, f64)> = nodes.iter().map(|n| (n.1, n.2)).collect();
    let ordered: Vec<(u64, f64, f64)> = hilbert::sort_order(&points)
        .iter()
        .map(|&i| {
            let (id, x, y) = nodes[i];
            let (lat, lon) = tm.convert(x, y);
            (id, lat, lon)
        })
        .collect();

    let (graph, skipped) = Graph::build(&ordered, &links);
    println!("노드 {} 간선 {} 건너뛴 링크 {}", graph.node_count(), graph.edge_count(), skipped);

    // 그래프 파일 저장
    let output = Path::new(GRAPH_FILE_LOCATION);
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    graph.write(output).unwrap();
    println!("저장 {} {}바이트", output.display(), std::fs::metadata(output).unwrap().len());

    // 저장한 파일을 mmap으로 열기
    let started = Instant::now();
    let file = GraphFile::open(output).unwrap();
    println!("열기 노드 {} 간선 {} ({:.1}ms)", file.node_count(), file.edge_count(), started.elapsed().as_secs_f64() * 1000.0);

    // 링크 1160059701 공원로 길이
    let e = file.link_id().iter().position(|&id| id == 1160059701).unwrap();
    println!("링크 1160059701 길이 {}", file.length()[e]);
    // 교차로 2290046502 능안교(동측) 위경도
    let v = file.node_id().iter().position(|&id| id == 2290046502).unwrap();
    println!("교차로 2290046502 위도 {:.8} 경도 {:.8}", file.lat()[v], file.lon()[v]);

    // 서울역 -> 강남역사거리, 서울역 -> 부산역(부산1호선)
    for (name, from_id, to_id) in [("서울역 -> 강남역사거리", 1010009300, 1210034101), ("서울역 -> 부산역", 1010009300, 1320000204)] {
        let from = file.node_id().iter().position(|&id| id == from_id).unwrap() as u32;
        let to = file.node_id().iter().position(|&id| id == to_id).unwrap() as u32;
        let started = Instant::now();
        let result = dijkstra::shortest_path(file.first_out(), file.head(), file.length(), from, to);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        match result {
            Some((dist, edges, _)) => println!("{} {:.0}m 링크 {}개 ({:.0}ms)", name, dist, edges.len(), elapsed),
            None => println!("{} 경로 없음 ({:.0}ms)", name, elapsed),
        }
    }
}
