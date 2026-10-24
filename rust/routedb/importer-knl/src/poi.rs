use std::fs::File;
use std::path::Path;

use routedb_core::graph::PoiInput;

fn reader(path: &Path) -> (csv::Reader<File>, csv::StringRecord) {
    let mut reader = csv::Reader::from_path(path).expect("CSV를 열 수 없음");
    let headers = reader.headers().expect("CSV 머리글을 읽을 수 없음").clone();
    (reader, headers)
}

fn col(headers: &csv::StringRecord, name: &str) -> usize {
    headers
        .iter()
        .position(|h| h.trim_start_matches('\u{feff}') == name)
        .expect("CSV에 컬럼이 없음")
}

/// 단속구분 문자열을 비트로 변환한다. 1 속도, 2 신호, 3 통행위반, 4 불법주정차, 99 기타이며 "1+2"처럼 여러 개면 비트를 합친다.
pub fn enforce_bits(text: &str) -> u8 {
    text.split('+')
        .map(|c| match c.trim() {
            "1" => 1,
            "2" => 2,
            "3" => 4,
            "4" => 8,
            _ => 16,
        })
        .fold(0, |a, b| a | b)
}

/// 카메라_링크.csv에서 행별 LINK_ID, 카메라번호, 링크비율, 제한속도, 단속구분을 추출한다.
pub fn read_cameras(path: &Path) -> Vec<PoiInput> {
    let (mut reader, headers) = reader(path);
    let [link, id, at, limit, enforce] =
        ["LINK_ID", "카메라번호", "링크비율", "제한속도", "단속구분"].map(|n| col(&headers, n));
    reader
        .records()
        .map(|r| {
            let r = r.expect("CSV 행을 읽을 수 없음");
            PoiInput {
                link_id: r[link].parse().expect("CSV 값이 숫자가 아님"),
                kind: 1,
                id: r[id].parse().expect("CSV 값이 숫자가 아님"),
                at: (r[at].parse::<f64>().expect("CSV 값이 숫자가 아님") * 10_000.0).round() as u16,
                limit: r[limit].parse().expect("CSV 값이 숫자가 아님"),
                enforce: enforce_bits(&r[enforce]),
            }
        })
        .collect()
}

/// 과속방지턱_링크.csv에서 행별 LINK_ID, 지점번호, 링크비율, 연속형을 추출한다. 연속형이면 enforce가 1이다.
pub fn read_bumps(path: &Path) -> Vec<PoiInput> {
    let (mut reader, headers) = reader(path);
    let [link, id, at, cont] =
        ["LINK_ID", "지점번호", "링크비율", "연속형"].map(|n| col(&headers, n));
    reader
        .records()
        .map(|r| {
            let r = r.expect("CSV 행을 읽을 수 없음");
            PoiInput {
                link_id: r[link].parse().expect("CSV 값이 숫자가 아님"),
                kind: 2,
                id: r[id].parse().expect("CSV 값이 숫자가 아님"),
                at: (r[at].parse::<f64>().expect("CSV 값이 숫자가 아님") * 10_000.0).round() as u16,
                limit: 0,
                enforce: (&r[cont] == "Y") as u8,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_poi_rows() {
        let cameras = read_cameras(&crate::paths::camera_links());
        let bumps = read_bumps(&crate::paths::bump_links());
        assert_eq!(cameras.len(), 55_899);
        assert_eq!(bumps.len(), 64_662);
        // 첫 카메라는 봉오대로 통행위반 단속, 링크 1670032800의 0.778 지점
        assert_eq!(
            (
                cameras[0].link_id,
                cameras[0].id,
                cameras[0].at,
                cameras[0].enforce
            ),
            (1670032800, 2, 7778, 4)
        );
        assert_eq!(enforce_bits("1+2"), 3);
        assert_eq!(enforce_bits("99"), 16);
        assert_eq!(
            (bumps[0].link_id, bumps[0].id, bumps[0].at, bumps[0].enforce),
            (2520185302, 0, 3952, 0)
        );
    }
}
