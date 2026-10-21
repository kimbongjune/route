mod coord;
mod moct;

use std::path::Path;
use std::time::Instant;

use routedb_core::file::GraphFile;
use routedb_core::graph::{Graph, LinkInput};
use routedb_core::hierarchy;
use routedb_core::snap::LinkIndex;
use routedb_core::{dijkstra, hilbert};

use coord::TmToWgs84;
use moct::MOCT_SHPFILE_FOLDER_LOCATION;

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";
const CCH_STRUCT_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-struct";
const CCH_METRIC_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-metric";

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

    // 링크 좌표를 TM에서 위경도로 변환
    let links: Vec<LinkInput> = links
        .into_iter()
        .map(|mut l| {
            l.geometry = l.geometry.iter().map(|&(x, y)| tm.convert(x, y)).collect();
            l
        })
        .collect();

    let (graph, skipped) = Graph::build(&ordered, &links);
    println!("노드 {} 간선 {} 링크 좌표 {} 건너뛴 링크 {}", graph.node_count(), graph.edge_count(), graph.geom_count(), skipped);

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

    // CCH 생성과 번들 저장
    let started = Instant::now();
    let (cch, metric) = hierarchy::build(file.first_out(), file.head(), file.length(), file.lat(), file.lon());
    println!("CCH 간선 {} ({:.1}초)", cch.cch_arc_count(), started.elapsed().as_secs_f64());
    cch.save_struct(Path::new(CCH_STRUCT_LOCATION)).unwrap();
    metric.save(Path::new(CCH_METRIC_LOCATION)).unwrap();
    println!("저장 {} {}바이트, {} {}바이트", CCH_STRUCT_LOCATION, std::fs::metadata(CCH_STRUCT_LOCATION).unwrap().len(),
             CCH_METRIC_LOCATION, std::fs::metadata(CCH_METRIC_LOCATION).unwrap().len());

    // 링크 선분 R-tree 생성과 서울역 좌표 스냅
    let started = Instant::now();
    let index = LinkIndex::build(file.geom_first(), file.geom_lat(), file.geom_lon());
    println!("R-tree 선분 {} ({:.0}ms)", index.len(), started.elapsed().as_secs_f64() * 1000.0);
    let started = Instant::now();
    let snap = index.nearest(37.5547, 126.9707).unwrap();
    println!("서울역 37.5547, 126.9707 -> 링크 {} 거리 {:.1}m 위치 {:.6}, {:.6} ({:.2}ms)",
             file.link_id()[snap.edge as usize], snap.distance_m, snap.lat, snap.lon, started.elapsed().as_secs_f64() * 1000.0);

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
