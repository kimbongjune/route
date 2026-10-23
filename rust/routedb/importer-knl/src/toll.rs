use std::path::Path;

/// 요금지점_링크.csv 위치
pub const TOLL_LINKS_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\유료도로\요금지점_링크.csv";

/// 요금지점_링크.csv에서 행별 LINK_ID와 영업소코드를 추출한다.
pub fn read_toll_links(path: &Path) -> Vec<(u64, u32)> {
    let mut reader = csv::Reader::from_path(path).unwrap();
    let headers = reader.headers().unwrap().clone();
    let col = |name: &str| headers.iter().position(|h| h.trim_start_matches('\u{feff}') == name).unwrap();
    let (unit_col, link_col) = (col("영업소코드"), col("LINK_ID"));
    let mut tolls = Vec::new();
    for record in reader.records() {
        let record = record.unwrap();
        if let (Ok(unit), Ok(link)) = (record[unit_col].parse(), record[link_col].parse()) {
            tolls.push((link, unit));
        }
    }
    tolls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_toll_link_rows() {
        let tolls = read_toll_links(Path::new(TOLL_LINKS_LOCATION));
        assert_eq!(tolls.len(), 2450);
        assert!(tolls.contains(&(1410199400, 246)));
    }
}
