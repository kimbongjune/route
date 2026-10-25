//! 골목 입력. `maps/골목/<구역>/*_골목_노드.csv`, `*_골목_링크.csv`(scripts/alley/extract_alleys.py)를 읽어
//! 큰길 그래프(moct.rdb)와 따로 골목 파일(alley.rdb)을 만든다. 골목이 큰길에 닿는 자리는 포털로 기록한다

use std::collections::HashMap;
use std::path::Path;

use routedb_core::alley::{AlleyBuilder, AlleyLinkInput, PORTAL_IN, PORTAL_OUT};
use routedb_core::file::GraphFile;

/// 골목 노드. `at`가 있으면 표준노드링크 링크 `on_link`(와 반대 방향 `on_link_rev`) 위 위치 비율에 놓인 새 교차로이다
#[derive(Clone, Debug)]
pub struct AlleyNode {
    pub id: u64,
    pub lat: f64,
    pub lon: f64,
    pub on_link: Option<u64>,
    pub on_link_rev: Option<u64>,
    pub at: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct AlleyLink {
    pub id: u64,
    pub from: u64,
    pub to: u64,
    pub length: f64,
    pub name: String,
    /// WGS84 (위도, 경도)
    pub geometry: Vec<(f64, f64)>,
}

fn col(headers: &csv::StringRecord, name: &str) -> usize {
    headers
        .iter()
        .position(|h| h.trim_start_matches('\u{feff}') == name)
        .unwrap_or_else(|| panic!("{name} 열이 없음"))
}

/// 노드 CSV를 읽는다
pub fn read_nodes(path: &Path) -> Vec<AlleyNode> {
    let mut reader = csv::Reader::from_path(path).expect("골목 노드 CSV를 열 수 없음");
    let headers = reader.headers().expect("헤더").clone();
    let (id, lat, lon, on, rev, at) = (
        col(&headers, "node_id"),
        col(&headers, "lat"),
        col(&headers, "lon"),
        col(&headers, "on_link"),
        col(&headers, "on_link_rev"),
        col(&headers, "at"),
    );
    let parse = |s: &str| -> Option<u64> { s.trim().parse().ok() };
    reader
        .records()
        .map(|r| {
            let r = r.expect("골목 노드 행");
            AlleyNode {
                id: r[id].parse().expect("node_id"),
                lat: r[lat].parse().expect("lat"),
                lon: r[lon].parse().expect("lon"),
                on_link: parse(&r[on]),
                on_link_rev: parse(&r[rev]),
                at: r[at].trim().parse().ok(),
            }
        })
        .collect()
}

/// 링크 CSV를 읽는다. wkt 열은 "lon lat lon lat ..."이다
pub fn read_links(path: &Path) -> Vec<AlleyLink> {
    let mut reader = csv::Reader::from_path(path).expect("골목 링크 CSV를 열 수 없음");
    let headers = reader.headers().expect("헤더").clone();
    let (id, from, to, length, name, wkt) = (
        col(&headers, "link_id"),
        col(&headers, "from"),
        col(&headers, "to"),
        col(&headers, "length_m"),
        col(&headers, "name"),
        col(&headers, "wkt"),
    );
    reader
        .records()
        .map(|r| {
            let r = r.expect("골목 링크 행");
            let nums: Vec<f64> = r[wkt]
                .split_whitespace()
                .map(|v| v.parse().expect("wkt 좌표"))
                .collect();
            AlleyLink {
                id: r[id].parse().expect("link_id"),
                from: r[from].parse().expect("from"),
                to: r[to].parse().expect("to"),
                length: r[length].parse().expect("length_m"),
                name: r[name].to_string(),
                geometry: nums.chunks(2).map(|c| (c[1], c[0])).collect(),
            }
        })
        .collect()
}

/// 폴더와 그 아래 폴더의 `*_골목_노드.csv`와 짝이 되는 `*_골목_링크.csv`를 모두 읽는다. 폴더가 없으면 빈 목록이다
pub fn read_all(dir: &Path) -> (Vec<AlleyNode>, Vec<AlleyLink>) {
    let (mut nodes, mut links) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (nodes, links);
    };
    let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            let (n, l) = read_all(&path);
            nodes.extend(n);
            links.extend(l);
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !name.ends_with("_골목_노드.csv") {
            continue;
        }
        let link_path = dir.join(name.replace("_골목_노드.csv", "_골목_링크.csv"));
        if link_path.exists() {
            println!("읽기 {}", path.display());
            nodes.extend(read_nodes(&path));
            links.extend(read_links(&link_path));
        }
    }
    (nodes, links)
}

/// 골목 파일 입력을 만든다. 노드 번호는 링크에 나오는 순서이고, 표준노드링크 노드는 그 노드에서 나가는 간선(비율 0)과
/// 들어오는 간선(비율 1)을, 링크 위 교차로는 그 링크와 반대 방향 링크의 위치 비율을 포털로 갖는다.
/// 그래프에 없는 노드나 링크를 가리키는 골목은 뺀다. 반환값은 (입력, 뺀 링크 수)
pub fn build(nodes: &[AlleyNode], links: &[AlleyLink], graph: &GraphFile) -> (AlleyBuilder, usize) {
    let moct_node: HashMap<u64, u32> = graph
        .node_id()
        .iter()
        .enumerate()
        .map(|(v, &id)| (id, v as u32))
        .collect();
    let moct_edge: HashMap<u64, u32> = graph
        .link_id()
        .iter()
        .enumerate()
        .map(|(e, &id)| (id, e as u32))
        .collect();
    let new_node: HashMap<u64, &AlleyNode> = nodes.iter().map(|n| (n.id, n)).collect();

    let mut b = AlleyBuilder::default();
    let mut index: HashMap<u64, u32> = HashMap::new();
    let mut moct_of: Vec<Option<u32>> = Vec::new();
    let mut names: HashMap<String, u32> = HashMap::new();
    b.names.push(String::new());
    names.insert(String::new(), 0);
    let mut skipped = 0;
    for l in links {
        let known = |id: u64| moct_node.contains_key(&id) || new_node.contains_key(&id);
        if !known(l.from) || !known(l.to) || l.geometry.len() < 2 {
            skipped += 1;
            continue;
        }
        let mut node_of = |id: u64, b: &mut AlleyBuilder| -> u32 {
            *index.entry(id).or_insert_with(|| {
                let (lat, lon, moct) = match moct_node.get(&id) {
                    Some(&v) => (graph.lat()[v as usize], graph.lon()[v as usize], Some(v)),
                    None => (new_node[&id].lat, new_node[&id].lon, None),
                };
                b.node_lat.push(lat);
                b.node_lon.push(lon);
                moct_of.push(moct);
                b.node_lat.len() as u32 - 1
            })
        };
        let (from, to) = (node_of(l.from, &mut b), node_of(l.to, &mut b));
        let name = *names.entry(l.name.clone()).or_insert_with(|| {
            b.names.push(l.name.clone());
            b.names.len() as u32 - 1
        });
        b.links.push(AlleyLinkInput {
            id: l.id,
            from,
            to,
            length: l.length as f32,
            name,
            points: l.geometry.clone(),
        });
    }
    // 포털
    let mut incoming: HashMap<u32, Vec<u32>> = HashMap::new();
    let wanted: std::collections::HashSet<u32> = moct_of.iter().flatten().copied().collect();
    let (first_out, head) = (graph.first_out(), graph.head());
    for (e, &h) in head.iter().enumerate() {
        if wanted.contains(&h) {
            incoming.entry(h).or_default().push(e as u32);
        }
    }
    let by_index: HashMap<u32, u64> = index.iter().map(|(&id, &v)| (v, id)).collect();
    b.portals = vec![Vec::new(); b.node_lat.len()];
    for v in 0..b.node_lat.len() {
        let list = &mut b.portals[v];
        if let Some(m) = moct_of[v] {
            for e in first_out[m as usize]..first_out[m as usize + 1] {
                list.push((e, 0.0, PORTAL_OUT));
            }
            for &e in incoming.get(&m).map_or(&[][..], Vec::as_slice) {
                list.push((e, 1.0, PORTAL_IN));
            }
            continue;
        }
        let n = new_node[&by_index[&(v as u32)]];
        let Some(at) = n.at else { continue };
        if let Some(&e) = n.on_link.and_then(|l| moct_edge.get(&l)) {
            list.push((e, at as f32, PORTAL_OUT | PORTAL_IN));
        }
        if let Some(&e) = n.on_link_rev.and_then(|l| moct_edge.get(&l)) {
            list.push((e, (1.0 - at) as f32, PORTAL_OUT | PORTAL_IN));
        }
    }
    (b, skipped)
}
