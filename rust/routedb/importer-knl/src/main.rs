mod coord;
mod moct;
mod toll;

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use routedb_core::file::GraphFile;
use routedb_core::graph::{Graph, LinkInput};
use routedb_core::hierarchy;
use routedb_core::linegraph::{self, LineGraph, LineGraphInput, TurnRule};
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
    let node_types: HashMap<u64, u16> = moct::read_node_types(folder).into_iter().collect();
    let turns = moct::read_turns(folder);
    println!("회전정보 {}", turns.len());

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

    let (mut graph, skipped) = Graph::build(&ordered, &links);
    println!("노드 {} 간선 {} 링크 좌표 {} 건너뛴 링크 {}", graph.node_count(), graph.edge_count(), graph.geom_count(), skipped);

    // 요금 지점 링크의 영업소코드
    let tolls = toll::read_toll_links(Path::new(toll::TOLL_LINKS_LOCATION));
    let missing = graph.set_tolls(&tolls);
    let toll_edges = (0..graph.edge_count()).filter(|&e| !graph.tolls_of(e).is_empty()).count();
    println!("요금 지점 행 {} 그래프에 없는 링크 행 {} 요금 지점 간선 {} 영업소코드 {}", tolls.len(), missing, toll_edges, graph.toll_unit.len());
    // 통과제한
    let count = |v: &[u16]| v.iter().filter(|&&x| x > 0).count();
    let mut veh = [0usize; 7];
    for &c in &graph.rest_veh {
        veh[c as usize] += 1;
    }
    println!("통과제한 높이 {} 하중 {} 통행제한차량 {:?}", count(&graph.rest_h), count(&graph.rest_w), veh);

    // 회전정보를 간선 번호의 회전 규칙으로 변환. 002, 003, 101, 102, 103은 금지, 011은 허용 U턴이다
    let edge_of: HashMap<u64, u32> = graph.link_id.iter().enumerate().map(|(e, &id)| (id, e as u32)).collect();
    let rule = |t: &moct::Turn| Some(TurnRule { st: *edge_of.get(&t.st_link)?, ed: *edge_of.get(&t.ed_link)? });
    let forbidden: Vec<TurnRule> = turns.iter().filter(|t| matches!(t.turn_type, 2 | 3 | 101 | 102 | 103)).filter_map(rule).collect();
    let uturn: Vec<TurnRule> = turns.iter().filter(|t| t.turn_type == 11).filter_map(rule).collect();
    println!("금지 회전 {} 허용 U턴 {}", forbidden.len(), uturn.len());

    // 교차로 그룹
    let crossing: Vec<bool> = graph.node_id.iter().map(|id| node_types[id] == 101).collect();
    // 회전 규칙마다 진입 링크 도착 노드에서 진출 링크 출발 노드까지 5개 이하 링크 경로를 찾아 한 그룹으로 묶는다
    let tail = linegraph::tails(&graph.first_out);
    // 경로가 없는 허용 U턴은 진입 링크 끝에서 진출 링크 시작으로 바로 건너가는 통행으로 만든다
    let (mut direct, mut no_path) = (0, 0);
    let mut joined: Vec<Vec<u32>> = Vec::new();
    let mut bridges: Vec<TurnRule> = Vec::new();
    for (r, is_uturn) in forbidden.iter().map(|r| (r, false)).chain(uturn.iter().map(|r| (r, true))) {
        match linegraph::rule_path(&graph.first_out, &graph.head, &graph.length, &tail, r.st, r.ed, 5) {
            Some(p) if p.len() == 1 => direct += 1,
            Some(p) => joined.push(p),
            None => {
                no_path += 1;
                if is_uturn {
                    bridges.push(TurnRule { st: r.st, ed: r.ed });
                }
            }
        }
    }
    println!("회전 규칙 경로: 바로 이어짐 {} 경로로 묶음 {} 경로 없음 {} (허용 U턴 {})", direct, joined.len(), no_path, bridges.len());
    let group = linegraph::intersection_groups(&graph.first_out, &graph.head, &graph.length, &crossing, 25.0, &joined);
    let internal = linegraph::internal_edges(&graph.first_out, &graph.head, &group);
    let group_count = group.iter().filter(|&&g| g != u32::MAX).max().map_or(0, |g| g + 1);
    let mut group_size = vec![0usize; group_count as usize];
    for &g in group.iter().filter(|&&g| g != u32::MAX) {
        group_size[g as usize] += 1;
    }
    println!("교차로 그룹 {} 내부 링크 {} 가장 큰 그룹 노드 {}", group_count, internal.iter().filter(|&&b| b).count(),
             group_size.iter().max().unwrap_or(&0));

    // line graph 생성
    let started = Instant::now();
    let geom_xy = linegraph::planar(&graph.geom_lat, &graph.geom_lon);
    let (start_dir, end_dir) = linegraph::link_directions(&graph.geom_first, &graph.geom_lat, &graph.geom_lon);
    let lg_input = LineGraphInput {
        first_out: &graph.first_out,
        head: &graph.head,
        length: &graph.length,
        group: &group,
        internal: &internal,
        geom_first: &graph.geom_first,
        geom_xy: &geom_xy,
        start_dir: &start_dir,
        end_dir: &end_dir,
        forbidden: &forbidden,
        uturn: &uturn,
        bridges: &bridges,
    };
    let (lg, stats) = LineGraph::build(&lg_input);
    println!("line graph 링크 {} 통행 {} 전개 통행 {} 금지 적용 {} U턴 허용 {} U턴 제거 {} 막다른 길 U턴 {} 금지보다 U턴 우선 {} 건너가는 통행 {} ({:.1}초)",
             lg.node_count(), stats.transitions, stats.expanded, stats.forbidden_applied, stats.uturn_allowed, stats.uturn_removed,
             stats.uturn_deadend, stats.forbidden_overridden, stats.bridged, started.elapsed().as_secs_f64());
    println!("걸린 회전 규칙: 금지 {}/{} 허용 U턴 {}/{}", stats.forbidden_rules_hit, forbidden.len(), stats.uturn_rules_hit, uturn.len());
    // 걸리지 않은 금지를 경로 없음과 교차로 내부 링크 포함으로 나눈다
    let (mut missed_no_path, mut missed_internal, mut missed_other) = (0, 0, 0);
    for &i in &stats.forbidden_missed {
        let r = &forbidden[i];
        if linegraph::rule_path(&graph.first_out, &graph.head, &graph.length, &tail, r.st, r.ed, 5).is_none() {
            missed_no_path += 1;
        } else if internal[r.st as usize] || internal[r.ed as usize] {
            missed_internal += 1;
        } else {
            missed_other += 1;
        }
    }
    println!("걸리지 않은 금지: 경로 없음 {} 진입 또는 진출이 내부 링크 {} 그 밖 {}", missed_no_path, missed_internal, missed_other);

    // 그래프 파일 저장
    let output = Path::new(GRAPH_FILE_LOCATION);
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    graph.write(&lg, output).unwrap();
    println!("저장 {} {}바이트", output.display(), std::fs::metadata(output).unwrap().len());

    // 저장한 파일을 mmap으로 열기
    let started = Instant::now();
    let file = GraphFile::open(output).unwrap();
    println!("열기 노드 {} 간선 {} 통행 {} 경유 링크 {} ({:.1}ms)", file.node_count(), file.edge_count(), file.turn_count(), file.via_count(),
             started.elapsed().as_secs_f64() * 1000.0);

    // 링크 1160059701 공원로 길이
    let e = file.link_id().iter().position(|&id| id == 1160059701).unwrap();
    println!("링크 1160059701 길이 {}", file.length()[e]);
    // 교차로 2290046502 능안교(동측) 위경도
    let v = file.node_id().iter().position(|&id| id == 2290046502).unwrap();
    println!("교차로 2290046502 위도 {:.8} 경도 {:.8}", file.lat()[v], file.lon()[v]);

    // line graph 다익스트라와 교차로 그래프 다익스트라 비교. 출발 링크 s에서 도착 링크 t까지의 line graph 거리에서 t 길이를 빼면 head[s]에서 tail[t]까지의 교차로 그래프 거리와 비교된다
    let compare = |s: u32, t: u32| -> (Option<(f64, usize, Vec<u32>)>, Option<(f64, usize)>) {
        let node = dijkstra::shortest_path(&graph.first_out, &graph.head, &graph.length, graph.head[s as usize], tail[t as usize])
            .map(|(d, edges, _)| (d, edges.len(), edges.iter().map(|&e| e as u32).collect()));
        let line = dijkstra::shortest_path(&lg.first_out, &lg.head, &lg.weight, s, t)
            .map(|(d, trans, _)| (d - graph.length[t as usize], trans.iter().map(|&x| lg.via_of(x).len() + 1).sum::<usize>() - 1));
        (node, line)
    };
    // 교차로 그래프 경로 s, edges.., t에서 line graph에 없는 통행을 찾는다. 내부 링크는 건너뛰고 진입 링크에서 진출 링크로의 통행이 있는지 본다
    let missing = |s: u32, edges: &[u32], t: u32| -> Vec<(u32, u32, &'static str)> {
        let mut path = vec![s];
        path.extend_from_slice(edges);
        path.push(t);
        let mut out = Vec::new();
        let mut a = path[0];
        for &b in &path[1..] {
            if internal[b as usize] {
                continue;
            }
            let found = (lg.first_out[a as usize]..lg.first_out[a as usize + 1]).any(|x| lg.head[x as usize] == b);
            if !found {
                let kind = if linegraph::is_uturn(&lg_input, a, b) { "U턴" } else { "금지 회전" };
                out.push((a, b, kind));
            }
            a = b;
        }
        out
    };
    for (name, from_id, to_id) in [("서울역 -> 강남역사거리", 1010009300, 1210034101), ("서울역 -> 부산역", 1010009300, 1320000204)] {
        // 출발 교차로에서 나가는 첫 링크, 도착 교차로로 들어오는 첫 링크. 교차로 내부 링크는 제외한다
        let from = graph.node_id.iter().position(|&id| id == from_id).unwrap();
        let to = graph.node_id.iter().position(|&id| id == to_id).unwrap() as u32;
        let s = (graph.first_out[from]..graph.first_out[from + 1]).find(|&e| !internal[e as usize]).unwrap();
        let t = graph.head.iter().enumerate().position(|(e, &h)| h == to && !internal[e]).unwrap() as u32;
        let started = Instant::now();
        let (node, line) = compare(s, t);
        println!("{} 교차로 그래프 {:?} line graph {:?} ({:.0}ms)", name, node.map(|(d, n, _)| (d, n)), line, started.elapsed().as_secs_f64() * 1000.0);
    }
    let mut seed = 12345u64;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) % n as u64) as u32
    };
    let (mut same, mut longer, mut shorter, mut none, mut total_node, mut total_line) = (0, 0, 0, 0, 0.0, 0.0);
    // 긴 쌍의 빠진 통행 분류: 출발 링크에서 막힘, 중간에서 막힘, 금지 회전 포함, U턴만
    let (mut at_start, mut at_middle, mut with_forbidden, mut uturn_only) = (0, 0, 0, 0);
    let pairs: Vec<(u32, u32)> = (0..100)
        .map(|_| loop {
            let (s, t) = (next(graph.edge_count()), next(graph.edge_count()));
            if !internal[s as usize] && !internal[t as usize] {
                break (s, t);
            }
        })
        .collect();
    let started = Instant::now();
    for &(s, t) in &pairs {
        match compare(s, t) {
            (Some((dn, _, edges)), Some((dl, _))) => {
                total_node += dn;
                total_line += dl;
                if (dl - dn).abs() < 0.01 {
                    same += 1
                } else if dl < dn {
                    // 건너가는 통행은 교차로 그래프에 없으므로 line graph가 짧을 수 있다
                    shorter += 1
                } else {
                    longer += 1;
                    let miss = missing(s, &edges, t);
                    if miss.first().is_some_and(|m| m.0 == s) { at_start += 1 } else { at_middle += 1 }
                    if miss.iter().any(|m| m.2 == "금지 회전") { with_forbidden += 1 } else { uturn_only += 1 }
                }
            }
            (Some(_), None) => none += 1,
            (None, _) => {}
        }
    }
    println!("무작위 100쌍: 같음 {} line graph가 김 {} line graph가 짧음 {} line graph 경로 없음 {} 교차로 그래프 합 {:.0}m line graph 합 {:.0}m ({:.1}초)",
             same, longer, shorter, none, total_node, total_line, started.elapsed().as_secs_f64());
    println!("  긴 쌍의 빠진 통행: 출발 링크에서 {} 중간에서 {} | 금지 회전 포함 {} U턴만 {}", at_start, at_middle, with_forbidden, uturn_only);

    // line graph 위에 CCH 생성과 번들 저장. 순서 계산 좌표는 링크 좌표의 가운데 점이다
    let started = Instant::now();
    let (geom_first, geom_lat, geom_lon) = (file.geom_first(), file.geom_lat(), file.geom_lon());
    let mid = |e: usize| (geom_first[e] as usize + geom_first[e + 1] as usize) / 2;
    let link_lat: Vec<f64> = (0..file.edge_count()).map(|e| geom_lat[mid(e)]).collect();
    let link_lon: Vec<f64> = (0..file.edge_count()).map(|e| geom_lon[mid(e)]).collect();
    let (cch, metric) = hierarchy::build(file.turn_first_out(), file.turn_head(), file.turn_weight(), &link_lat, &link_lon);
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

}
