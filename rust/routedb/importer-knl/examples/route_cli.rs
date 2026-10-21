// 출발 위경도와 도착 위경도를 받아 line graph CCH 번들로 경로와 거리를 출력한다.
// cargo run --release -p importer-knl --example route_cli -- 37.5547 126.9707 37.4979 127.0276
use std::path::Path;
use std::time::Instant;

use cch::bundle::{CchBundle, MetricBundle};
use cch::path::PathQuery;
use routedb_core::file::GraphFile;
use routedb_core::hierarchy::edges_of_node_path;
use routedb_core::snap::{LinkIndex, Snap};

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";
const CCH_STRUCT_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-struct";
const CCH_METRIC_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-metric";

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
    let cch = CchBundle::open(Path::new(CCH_STRUCT_LOCATION)).unwrap();
    let metric = MetricBundle::open(Path::new(CCH_METRIC_LOCATION)).unwrap();
    let (cch_v, metric_v) = (cch.view(), metric.view());
    let mut query = PathQuery::new(&cch_v);
    println!("그래프와 CCH 번들 열기, R-tree 생성 {:.1}초", started.elapsed().as_secs_f64());

    let started = Instant::now();
    let (turn_first_out, turn_head, turn_weight, length, link_id) = (file.turn_first_out(), file.turn_head(), file.turn_weight(), file.length(), file.link_id());
    // 가까운 간선 5개씩을 후보로 두고 25개 조합 중 출발·도착 스냅 거리와 경로 거리의 합이 가장 작은 조합을 고른다
    let froms = index.nearest_n(from_lat, from_lon, 5);
    let tos = index.nearest_n(to_lat, to_lon, 5);
    let mut result: Option<(&Snap, &Snap, f64, Vec<u64>)> = None;
    for from in &froms {
        for to in &tos {
            // 출발 링크에서 도착 링크까지 line graph 경로를 구하고, 통행마다 경유 내부 링크를 펼쳐 링크 열을 만든다
            let (from_frac, to_frac) = (fraction(&file, from), fraction(&file, to));
            let found = if from.edge == to.edge && from_frac <= to_frac {
                Some((length[from.edge as usize] * (to_frac - from_frac), vec![link_id[from.edge as usize]]))
            } else {
                query.path(&metric_v, from.edge, to.edge).map(|links| {
                    let turns = edges_of_node_path(turn_first_out, turn_head, turn_weight, &links);
                    let mut edges = vec![from.edge];
                    for &t in &turns {
                        edges.extend_from_slice(file.via_of(t));
                        edges.push(turn_head[t]);
                    }
                    let between: f64 = edges[1..edges.len() - 1].iter().map(|&e| length[e as usize]).sum();
                    let total = length[from.edge as usize] * (1.0 - from_frac) + between + length[to.edge as usize] * to_frac;
                    (total, edges.iter().map(|&e| link_id[e as usize]).collect())
                })
            };
            if let Some((total, links)) = found {
                let cost = from.distance_m + total + to.distance_m;
                if result.as_ref().is_none_or(|r| cost < r.0.distance_m + r.2 + r.1.distance_m) {
                    result = Some((from, to, total, links));
                }
            }
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
