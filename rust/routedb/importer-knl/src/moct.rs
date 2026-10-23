use std::path::Path;

use routedb_core::graph::LinkInput;

/// MOCT_NODE의 노드. 좌표는 TM이다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub id: u64,
    pub x: f64,
    pub y: f64,
}
use shapefile::Shape;
use shapefile::dbase::{FieldValue, Record};

pub const MOCT_SHPFILE_FOLDER_LOCATION: &str =
    r"C:\Users\nocdu\Desktop\dev\route\maps\[2026-09-14]NODELINKDATA";

/// shp파일에서 특정 필드를 문자열로 추출한다.
fn text_field<'a>(record: &'a Record, name: &str) -> Option<&'a str> {
    match record.get(name) {
        Some(FieldValue::Character(Some(s))) => Some(s.as_str()),
        _ => None,
    }
}

/// shp파일에서 특정 필드를 숫자로 추출한다.
fn numeric_field(record: &Record, name: &str) -> Option<f64> {
    match record.get(name) {
        Some(FieldValue::Numeric(Some(v))) => Some(*v),
        _ => None,
    }
}

/// MOCT_NODE.shp에서 노드별 NODE_ID와 TM 좌표(x, y)를 추출한다.
pub fn read_nodes(folder: &Path) -> Vec<Node> {
    let mut reader = shapefile::Reader::from_path(folder.join("MOCT_NODE.shp")).unwrap();
    let mut nodes = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result.expect("레코드를 읽을 수 없음");
        let Shape::Point(p) = shape else {
            panic!("노드가 Point 타입이 아님")
        };
        let id: u64 = text_field(&record, "NODE_ID")
            .expect("NODE_ID 필드가 없음")
            .parse()
            .expect("숫자 필드가 아님");
        nodes.push(Node { id, x: p.x, y: p.y });
    }
    nodes
}

/// MOCT_NODE.shp에서 노드별 NODE_ID와 NODE_TYPE을 추출한다.
pub fn read_node_types(folder: &Path) -> Vec<(u64, u16)> {
    let mut reader = shapefile::Reader::from_path(folder.join("MOCT_NODE.shp")).unwrap();
    let mut nodes = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (_, record) = result.expect("레코드를 읽을 수 없음");
        let id: u64 = text_field(&record, "NODE_ID")
            .expect("NODE_ID 필드가 없음")
            .parse()
            .expect("숫자 필드가 아님");
        let node_type: u16 = text_field(&record, "NODE_TYPE")
            .expect("NODE_TYPE 필드가 없음")
            .parse()
            .expect("숫자 필드가 아님");
        nodes.push((id, node_type));
    }
    nodes
}

/// TURNINFO.dbf의 회전정보 한 건
pub struct Turn {
    pub st_link: u64,
    pub ed_link: u64,
    pub turn_type: u16,
}

/// TURNINFO.dbf에서 회전정보별 ST_LINK, ED_LINK, TURN_TYPE을 추출한다.
pub fn read_turns(folder: &Path) -> Vec<Turn> {
    let mut reader = shapefile::dbase::Reader::from_path(folder.join("TURNINFO.dbf")).unwrap();
    let mut turns = Vec::new();
    for result in reader.iter_records() {
        let record = result.expect("레코드를 읽을 수 없음");
        let field = |name| {
            text_field(&record, name)
                .expect("필드가 없음")
                .parse()
                .expect("숫자 필드가 아님")
        };
        turns.push(Turn {
            st_link: field("ST_LINK"),
            ed_link: field("ED_LINK"),
            turn_type: text_field(&record, "TURN_TYPE")
                .expect("TURN_TYPE 필드가 없음")
                .parse()
                .expect("숫자 필드가 아님"),
        });
    }
    turns
}

/// MOCT_LINK.shp에서 링크별 LINK_ID, F_NODE, T_NODE, LENGTH, TM 좌표 목록 (x, y), 통과제한 REST_VEH, REST_W, REST_H, 제한속도 MAX_SPD, 도로등급 ROAD_RANK, 도로명 ROAD_NAME을 추출한다.
/// ROAD_USE=1 미사용 링크는 제외한다.
pub fn read_links(folder: &Path) -> Vec<LinkInput> {
    let mut reader = shapefile::Reader::from_path(folder.join("MOCT_LINK.shp")).unwrap();
    let mut links = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result.expect("레코드를 읽을 수 없음");
        if text_field(&record, "ROAD_USE") == Some("1") {
            continue;
        }
        let Shape::Polyline(line) = shape else {
            panic!("링크가 Polyline 타입이 아님")
        };
        let id: u64 = text_field(&record, "LINK_ID")
            .expect("LINK_ID 필드가 없음")
            .parse()
            .expect("숫자 필드가 아님");
        let from: u64 = text_field(&record, "F_NODE")
            .expect("F_NODE 필드가 없음")
            .parse()
            .expect("숫자 필드가 아님");
        let to: u64 = text_field(&record, "T_NODE")
            .expect("T_NODE 필드가 없음")
            .parse()
            .expect("숫자 필드가 아님");
        let length = numeric_field(&record, "LENGTH").expect("LENGTH 필드가 없음");
        let geometry = line.parts().iter().flatten().map(|p| (p.x, p.y)).collect();
        let rest_veh: u8 = text_field(&record, "REST_VEH")
            .expect("REST_VEH 필드가 없음")
            .trim()
            .parse()
            .expect("숫자 필드가 아님");
        let rest_w = numeric_field(&record, "REST_W").expect("REST_W 필드가 없음") as u16;
        let rest_h = numeric_field(&record, "REST_H").expect("REST_H 필드가 없음") as u16;
        let max_spd = numeric_field(&record, "MAX_SPD").expect("MAX_SPD 필드가 없음") as u16;
        let road_rank: u8 = text_field(&record, "ROAD_RANK")
            .expect("ROAD_RANK 필드가 없음")
            .trim()
            .parse()
            .expect("숫자 필드가 아님");
        // 도로명이 없는 링크는 "-"이다
        let road_name = text_field(&record, "ROAD_NAME").unwrap_or("").trim();
        let road_name = if road_name == "-" {
            String::new()
        } else {
            road_name.to_string()
        };
        links.push(LinkInput {
            id,
            from,
            to,
            length,
            geometry,
            rest_veh,
            rest_w,
            rest_h,
            max_spd,
            road_rank,
            road_name,
        });
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_moct_link_data() {
        let real_count = 1_562_356;
        let moct_link_data_file = Path::new(MOCT_SHPFILE_FOLDER_LOCATION).join("MOCT_LINK.shp");
        let moct_link_reader = shapefile::Reader::from_path(moct_link_data_file).unwrap();
        assert_eq!(real_count, moct_link_reader.shape_count().unwrap());
    }

    #[test]
    fn count_moct_node_data() {
        let real_count = 1_183_329;
        let moct_node_data_file = Path::new(MOCT_SHPFILE_FOLDER_LOCATION).join("MOCT_NODE.shp");
        let moct_node_reader = shapefile::Reader::from_path(moct_node_data_file).unwrap();
        assert_eq!(real_count, moct_node_reader.shape_count().unwrap());
    }

    #[test]
    fn check_korean_field_read() {
        let moct_link_data_file = Path::new(MOCT_SHPFILE_FOLDER_LOCATION).join("MOCT_LINK.shp");
        let mut moct_link_reader = shapefile::Reader::from_path(moct_link_data_file).unwrap();
        let (_, record) = moct_link_reader
            .iter_shapes_and_records()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(Some("1160059701"), text_field(&record, "LINK_ID"));
        assert_eq!(Some("공원로"), text_field(&record, "ROAD_NAME"));
    }
}
