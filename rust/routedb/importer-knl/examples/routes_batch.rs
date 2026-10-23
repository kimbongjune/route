// 출발·도착 위경도 쌍을 CSV로 받아 프로파일별 경로의 링크 열을 줄마다 출력한다. 프로파일 확인용이다.
// cargo run --release -p importer-knl --example routes_batch -- pairs.csv free > routes.txt
use std::path::Path;

use cch::bundle::{CchBundle, MetricBundle};
use cch::path::PathQuery;
use routedb_core::file::GraphFile;
use routedb_core::hierarchy::edges_of_node_path;
use routedb_core::snap::LinkIndex;

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";
const CCH_STRUCT_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-struct";
const METRIC_LOCATIONS: [(&str, &str); 3] = [
    ("distance", r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-metric"),
    ("time", r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-metric-time"),
    ("free", r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-metric-free"),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [pairs_path, profile] = &args[..] else {
        eprintln!("사용법: routes_batch 쌍CSV distance|time|free");
        std::process::exit(1);
    };
    let metric_location = METRIC_LOCATIONS.iter().find(|(n, _)| n == profile).expect("프로파일은 distance, time, free 중 하나").1;
    let file = GraphFile::open(Path::new(GRAPH_FILE_LOCATION)).unwrap();
    let index = LinkIndex::build(file.geom_first(), file.geom_lat(), file.geom_lon());
    let cch = CchBundle::open(Path::new(CCH_STRUCT_LOCATION)).unwrap();
    let metric = MetricBundle::open(Path::new(metric_location)).unwrap();
    let (cch_v, metric_v) = (cch.view(), metric.view());
    let mut query = PathQuery::new(&cch_v);
    let (turn_first_out, turn_head, turn_weight, link_id) = (file.turn_first_out(), file.turn_head(), file.turn_weight(), file.link_id());

    for line in std::fs::read_to_string(pairs_path).unwrap().lines() {
        let v: Vec<f64> = line.split(',').map(|x| x.trim().parse().unwrap()).collect();
        // 출발·도착 후보 5개씩 중 경로가 있는 첫 조합
        let froms = index.nearest_n(v[0], v[1], 5);
        let tos = index.nearest_n(v[2], v[3], 5);
        let mut found = None;
        'outer: for from in &froms {
            for to in &tos {
                if let Some(nodes) = query.path(&metric_v, from.edge, to.edge) {
                    let turns = edges_of_node_path(turn_first_out, turn_head, turn_weight, &nodes);
                    let mut edges = vec![from.edge];
                    for &t in &turns {
                        edges.extend_from_slice(file.via_of(t));
                        edges.push(turn_head[t]);
                    }
                    found = Some(edges);
                    break 'outer;
                }
            }
        }
        match found {
            Some(edges) => println!("{}", edges.iter().map(|&e| link_id[e as usize].to_string()).collect::<Vec<_>>().join(" ")),
            None => println!(),
        }
    }
}
