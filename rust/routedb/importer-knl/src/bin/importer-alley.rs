//! 골목 파일(alley.rdb)을 만든다. maps/골목 아래 시도별 CSV와 큰길 그래프 moct.rdb를 읽어 그래프 폴더에 쓴다.
//! 큰길 그래프를 다시 만들면 간선 번호가 바뀔 수 있으므로 이것도 다시 실행한다
//!
//! cargo run --release -p importer-knl --bin importer-alley -- [골목 폴더. 기본 maps/골목]

#[path = "../alley.rs"]
mod alley;
#[path = "../paths.rs"]
#[allow(dead_code)]
mod paths;

use std::time::Instant;

use routedb_core::alley::AlleyFile;
use routedb_core::file::GraphFile;

fn main() {
    let started = Instant::now();
    let dir = std::env::args()
        .nth(1)
        .map_or_else(paths::alley_dir, std::path::PathBuf::from);
    let (nodes, links) = alley::read_all(&dir);
    println!(
        "골목 CSV 노드 {} 링크 {} ({:.1}초)",
        nodes.len(),
        links.len(),
        started.elapsed().as_secs_f64()
    );
    let graph = GraphFile::open(&paths::graph_file()).expect("moct.rdb를 열지 못함");
    let started = Instant::now();
    let (builder, skipped) = alley::build(&nodes, &links, &graph);
    let portals: usize = builder.portals.iter().map(Vec::len).sum();
    let portal_nodes = builder.portals.iter().filter(|p| !p.is_empty()).count();
    println!(
        "골목 노드 {} 링크 {} 뺀 링크 {} 포털 노드 {} 포털 {} 이름 {} ({:.1}초)",
        builder.node_lat.len(),
        builder.links.len(),
        skipped,
        portal_nodes,
        portals,
        builder.names.len(),
        started.elapsed().as_secs_f64()
    );
    let path = paths::alley_file();
    let started = Instant::now();
    builder.write(&path).expect("골목 파일을 쓰지 못함");
    let file = AlleyFile::open(&path).expect("골목 파일을 다시 열지 못함");
    println!(
        "저장 {} {}바이트 노드 {} 간선 {} ({:.1}초)",
        path.display(),
        std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        file.node_count(),
        file.edge_count(),
        started.elapsed().as_secs_f64()
    );
}
