use std::path::Path;

use shapefile::dbase::{FieldValue, Record};
use shapefile::Shape;

pub const MOCT_SHPFILE_FOLDER_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\[2026-09-14]NODELINKDATA";

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
pub fn read_nodes(folder: &Path) -> Vec<(u64, f64, f64)> {
    let mut reader = shapefile::Reader::from_path(folder.join("MOCT_NODE.shp")).unwrap();
    let mut nodes = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result.unwrap();
        let Shape::Point(p) = shape else { panic!("노드가 Point 타입이 아님") };
        let id: u64 = text_field(&record, "NODE_ID").unwrap().parse().unwrap();
        nodes.push((id, p.x, p.y));
    }
    nodes
}

/// MOCT_LINK.shp에서 링크별 LINK_ID, F_NODE, T_NODE, LENGTH를 추출한다.
pub fn read_links(folder: &Path) -> Vec<(u64, u64, u64, f64)> {
    let mut reader = shapefile::Reader::from_path(folder.join("MOCT_LINK.shp")).unwrap();
    let mut links = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (_, record) = result.unwrap();
        let link: u64 = text_field(&record, "LINK_ID").unwrap().parse().unwrap();
        let f: u64 = text_field(&record, "F_NODE").unwrap().parse().unwrap();
        let t: u64 = text_field(&record, "T_NODE").unwrap().parse().unwrap();
        let length = numeric_field(&record, "LENGTH").unwrap();
        links.push((link, f, t, length));
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
        let (_, record) = moct_link_reader.iter_shapes_and_records().next().unwrap().unwrap();
        assert_eq!(Some("1160059701"), text_field(&record, "LINK_ID"));
        assert_eq!(Some("공원로"), text_field(&record, "ROAD_NAME"));
    }
}
