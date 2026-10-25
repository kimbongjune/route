use std::path::Path;

use routedb_core::graph::TollInput;

/// 요금지점_링크.csv에서 행별 LINK_ID와 영업소코드를 추출한다.
pub fn read_toll_links(path: &Path) -> Vec<TollInput> {
    let mut reader = csv::Reader::from_path(path).expect("CSV를 열 수 없음");
    let headers = reader.headers().expect("CSV 머리글을 읽을 수 없음").clone();
    let col = |name: &str| {
        headers
            .iter()
            .position(|h| h.trim_start_matches('\u{feff}') == name)
            .expect("CSV에 컬럼이 없음")
    };
    let (unit_col, link_col) = (col("영업소코드"), col("LINK_ID"));
    let mut tolls = Vec::new();
    for record in reader.records() {
        let record = record.expect("CSV 행을 읽을 수 없음");
        if let (Ok(unit), Ok(link)) = (record[unit_col].parse(), record[link_col].parse()) {
            tolls.push(TollInput::new(link, unit));
        }
    }
    tolls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_toll_link_rows() {
        let tolls = read_toll_links(&crate::paths::toll_links());
        assert_eq!(tolls.len(), 4502);
        assert!(tolls.contains(&TollInput::new(1410199400, 246)));
    }
}
