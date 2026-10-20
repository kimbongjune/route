// 출발 위경도와 도착 위경도를 받아 경로와 거리를 출력한다.
// cargo run --release -p importer-knl --example route_cli -- 37.5547 126.9707 37.4979 127.0276
use std::path::Path;
use std::time::Instant;

use routedb_core::astar;
use routedb_core::file::GraphFile;
use routedb_core::snap::{LinkIndex, Snap};

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";

/// 간선 e의 출발 교차로 번호를 first_out에서 탐색한다.
fn tail_of(first_out: &[u32], e: usize) -> u32 {
    (first_out.partition_point(|&f| f as usize <= e) - 1) as u32
}

/// 간선 e의 평면 좌표상 길이(m)를 좌표 목록으로 계산한다.
fn planar_length(file: &GraphFile, e: usize) -> f64 {
    let (s, t) = (file.geom_first()[e] as usize, file.geom_first()[e + 1] as usize);
    let (lat, lon) = (file.geom_lat(), file.geom_lon());
    (s..t.saturating_sub(1))
        .map(|i| {
            let dy = (lat[i + 1] - lat[i]) * 111_320.0;
            let dx = (lon[i + 1] - lon[i]) * 111_320.0 * 0.809_017;
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

/// 스냅 위치가 간선 시작점에서 얼마나 떨어졌는지를 간선 길이 기준 비율(0~1)로 계산한다.
fn fraction(file: &GraphFile, snap: &Snap) -> f64 {
    let planar = planar_length(file, snap.edge as usize);
    if planar == 0.0 { 0.0 } else { (snap.offset_m / planar).clamp(0.0, 1.0) }
}

fn main() {
    let args: Vec<f64> = std::env::args().skip(1).map(|a| a.parse().expect("위경도는 숫자여야 함")).collect();
    let [from_lat, from_lon, to_lat, to_lon] = args[..] else {
        eprintln!("사용법: route_cli 출발위도 출발경도 도착위도 도착경도");
        std::process::exit(1);
    };

    let started = Instant::now();
    let file = GraphFile::open(Path::new(GRAPH_FILE_LOCATION)).unwrap();
    let index = LinkIndex::build(file.geom_first(), file.geom_lat(), file.geom_lon());
    println!("그래프 열기와 R-tree 생성 {:.1}초", started.elapsed().as_secs_f64());

    let started = Instant::now();
    let (first_out, head, length, link_id) = (file.first_out(), file.head(), file.length(), file.link_id());
    // 가까운 간선 5개씩을 후보로 두고 거리 합이 작은 순서로 경로가 있는 조합을 찾는다
    let froms = index.nearest_n(from_lat, from_lon, 5);
    let tos = index.nearest_n(to_lat, to_lon, 5);
    let mut pairs: Vec<(&Snap, &Snap)> = froms.iter().flat_map(|f| tos.iter().map(move |t| (f, t))).collect();
    pairs.sort_by(|a, b| (a.0.distance_m + a.1.distance_m).total_cmp(&(b.0.distance_m + b.1.distance_m)));

    let mut result = None;
    for (from, to) in pairs {
        // 출발은 스냅된 간선의 도착 교차로에서 시작하고 도착은 스냅된 간선의 출발 교차로에서 끝낸다
        let (from_frac, to_frac) = (fraction(&file, from), fraction(&file, to));
        if from.edge == to.edge && from_frac <= to_frac {
            result = Some((from, to, length[from.edge as usize] * (to_frac - from_frac), vec![link_id[from.edge as usize]]));
            break;
        }
        let start = head[from.edge as usize];
        let end = tail_of(first_out, to.edge as usize);
        if let Some((dist, edges, _)) = astar::shortest_path(first_out, head, length, file.lat(), file.lon(), start, end) {
            let total = length[from.edge as usize] * (1.0 - from_frac) + dist + length[to.edge as usize] * to_frac;
            let mut links = vec![link_id[from.edge as usize]];
            links.extend(edges.iter().map(|&e| link_id[e]));
            links.push(link_id[to.edge as usize]);
            result = Some((from, to, total, links));
            break;
        }
    }
    let Some((from, to, total, links)) = result else {
        println!("경로 없음");
        return;
    };
    println!("출발 스냅 링크 {} 거리 {:.1}m | 도착 스냅 링크 {} 거리 {:.1}m",
             link_id[from.edge as usize], from.distance_m, link_id[to.edge as usize], to.distance_m);
    println!("거리 {:.0}m 링크 {}개 ({:.0}ms)", total, links.len(), started.elapsed().as_secs_f64() * 1000.0);
    println!("{}", links.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(" "));
}
