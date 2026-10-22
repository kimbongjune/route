use std::collections::HashMap;
use std::path::Path;

use routedb_core::graph::LinkInput;

/// 휴게소_연결링크.csv 위치
pub const REST_LINKS_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\유료도로\휴게소_연결링크.csv";
/// 휴게소_연결노드.csv 위치
pub const REST_NODES_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\유료도로\휴게소_연결노드.csv";

/// 휴게소 연결 링크. 표준노드링크를 나눈 조각이면 원본 LINK_ID와 원본 안의 순서가 있다.
pub struct RestLink {
    pub link: LinkInput,
    pub origin: Option<(u64, u32)>,
    pub toll_unit: Option<u32>,
}

fn reader(path: &Path) -> (csv::Reader<std::fs::File>, csv::StringRecord) {
    let mut reader = csv::Reader::from_path(path).unwrap();
    let headers = reader.headers().unwrap().clone();
    (reader, headers)
}

fn col(headers: &csv::StringRecord, name: &str) -> usize {
    headers.iter().position(|h| h.trim_start_matches('\u{feff}') == name).unwrap()
}

/// 휴게소_연결링크.csv에서 링크별 LINK_ID, F_NODE, T_NODE, LENGTH, TM 좌표, 원본 LINK_ID와 순서, 영업소코드를 추출한다.
pub fn read_rest_links(path: &Path) -> Vec<RestLink> {
    let (mut reader, headers) = reader(path);
    let [id, from, to, length, origin, order, unit, coords] =
        ["LINK_ID", "F_NODE", "T_NODE", "LENGTH", "원본LINK_ID", "순서", "영업소코드", "좌표"].map(|name| col(&headers, name));
    let mut links = Vec::new();
    for record in reader.records() {
        let record = record.unwrap();
        let geometry = record[coords]
            .split(';')
            .map(|p| {
                let (x, y) = p.split_once(' ').unwrap();
                (x.parse().unwrap(), y.parse().unwrap())
            })
            .collect();
        links.push(RestLink {
            link: LinkInput {
                id: record[id].parse().unwrap(),
                from: record[from].parse().unwrap(),
                to: record[to].parse().unwrap(),
                length: record[length].parse().unwrap(),
                geometry,
            },
            origin: record[origin].parse().ok().map(|o| (o, record[order].parse().unwrap())),
            toll_unit: record[unit].parse().ok(),
        });
    }
    links
}

/// 휴게소_연결노드.csv에서 노드별 NODE_ID와 TM 좌표(x, y)를 추출한다.
pub fn read_rest_nodes(path: &Path) -> Vec<(u64, f64, f64)> {
    let (mut reader, headers) = reader(path);
    let [id, x, y] = ["NODE_ID", "x", "y"].map(|name| col(&headers, name));
    reader
        .records()
        .map(|r| {
            let r = r.unwrap();
            (r[id].parse().unwrap(), r[x].parse().unwrap(), r[y].parse().unwrap())
        })
        .collect()
}

/// 나눈 표준노드링크 링크의 원본 LINK_ID -> (첫 조각 LINK_ID, 마지막 조각 LINK_ID)
pub fn piece_ends(links: &[RestLink]) -> HashMap<u64, (u64, u64)> {
    let mut ends: HashMap<u64, (u32, u64, u32, u64)> = HashMap::new();
    for l in links {
        if let Some((origin, order)) = l.origin {
            let e = ends.entry(origin).or_insert((order, l.link.id, order, l.link.id));
            if order < e.0 {
                (e.0, e.1) = (order, l.link.id);
            }
            if order > e.2 {
                (e.2, e.3) = (order, l.link.id);
            }
        }
    }
    ends.into_iter().map(|(o, (_, first, _, last))| (o, (first, last))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_rest_area_rows() {
        let links = read_rest_links(Path::new(REST_LINKS_LOCATION));
        let nodes = read_rest_nodes(Path::new(REST_NODES_LOCATION));
        assert_eq!(links.len(), 42);
        assert_eq!(nodes.len(), 22);
        // 요금소 링크는 행담도 4개, 덕평 2개, 진영 1개
        let units: Vec<u32> = links.iter().filter_map(|l| l.toll_unit).collect();
        assert_eq!(units.iter().filter(|&&u| u == 732).count(), 4);
        assert_eq!(units.iter().filter(|&&u| u == 220).count(), 2);
        assert_eq!(units.iter().filter(|&&u| u == 758).count(), 1);
        // 나눈 원본 링크 9개. 조각은 원본의 F_NODE에서 시작해 T_NODE에서 끝난다
        let ends = piece_ends(&links);
        assert_eq!(ends.len(), 9);
        let (first, last) = ends[&3851176701];
        assert_ne!(first, last);
    }
}
