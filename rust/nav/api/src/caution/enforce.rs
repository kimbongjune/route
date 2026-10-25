//! 구간단속. scripts/camera/match_links.py가 만든 카메라_링크.csv의 구간위치·구간짝(짝 카메라 관리번호)으로
//! 시점 카메라와 종점 카메라를 잇는다. 경로가 시점 카메라를 지나고 그 뒤에 짝인 종점 카메라를 지나면 두 카메라 사이를 구간단속 구간으로 본다

use std::collections::HashMap;
use std::io;
use std::path::Path;

use routedb_core::route::{PoiKind, RoutePoi};
use serde::Deserialize;

/// 카메라_링크.csv 한 행. 한 카메라가 링크 여러 개에 매칭되면 행도 여럿이다
#[derive(Deserialize)]
struct Row {
    #[serde(rename = "카메라번호")]
    camera: u32,
    #[serde(rename = "관리번호")]
    id: String,
    #[serde(rename = "제한속도")]
    limit: String,
    #[serde(rename = "구간위치")]
    position: String,
    #[serde(rename = "구간짝")]
    mate: String,
    #[serde(rename = "구간길이m")]
    length: String,
}

/// 시점 카메라 하나의 구간단속 정보
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionPair {
    /// 짝인 종점 카메라번호
    pub end: u32,
    /// 원본 구간 길이(m)
    pub length_m: f64,
    /// 제한속도(km/h). 0은 없음
    pub limit: u8,
}

/// 경로가 지나는 구간단속 구간
#[derive(Clone, Debug, PartialEq)]
pub struct EnforceSection {
    pub from_m: f64,
    pub to_m: f64,
    pub limit: u8,
}

/// 시작·끝 거리가 둘 다 이 거리 안이면 같은 구간으로 본다(m). 차로마다 시점·종점 카메라가 따로 등록되어 있다
const SAME_SECTION_M: f64 = 100.0;

/// 시점 카메라번호에서 구간단속 정보
#[derive(Default)]
pub struct EnforceTable {
    pairs: HashMap<u32, SectionPair>,
}

impl EnforceTable {
    /// 카메라_링크.csv를 읽는다
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut reader = csv::Reader::from_path(path)?;
        let mut rows = Vec::new();
        for row in reader.deserialize::<Row>() {
            rows.push(row.map_err(io::Error::other)?);
        }
        let camera_of: HashMap<&str, u32> =
            rows.iter().map(|r| (r.id.as_str(), r.camera)).collect();
        let mut pairs = HashMap::new();
        for r in &rows {
            if r.position != "시점" {
                continue;
            }
            let Some(&end) = camera_of.get(r.mate.as_str()) else {
                continue;
            };
            pairs.insert(
                r.camera,
                SectionPair {
                    end,
                    length_m: r.length.trim().parse().unwrap_or(0.0),
                    limit: r.limit.trim().parse().unwrap_or(0),
                },
            );
        }
        Ok(Self { pairs })
    }

    pub fn from_pairs(pairs: HashMap<u32, SectionPair>) -> Self {
        Self { pairs }
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// 경로 지점 열에서 시점 카메라 뒤에 짝인 종점 카메라가 나오는 구간. 제한속도는 시점 카메라의 값이고,
    /// 시작·끝이 SAME_SECTION_M 안인 구간은 하나만 남긴다
    pub fn sections(&self, pois: &[RoutePoi]) -> Vec<EnforceSection> {
        let cameras: Vec<&RoutePoi> = pois.iter().filter(|p| p.kind == PoiKind::Camera).collect();
        let mut out = Vec::new();
        for (i, start) in cameras.iter().enumerate() {
            let Some(pair) = self.pairs.get(&start.id) else {
                continue;
            };
            if let Some(end) = cameras[i + 1..].iter().find(|c| c.id == pair.end) {
                let same = |x: &EnforceSection| {
                    (x.from_m - start.at_m).abs() < SAME_SECTION_M
                        && (x.to_m - end.at_m).abs() < SAME_SECTION_M
                };
                if out.iter().any(same) {
                    continue;
                }
                out.push(EnforceSection {
                    from_m: start.at_m,
                    to_m: end.at_m,
                    limit: pair.limit,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(id: u32, at_m: f64) -> RoutePoi {
        RoutePoi {
            kind: PoiKind::Camera,
            id,
            at_m,
            lat: 0.0,
            lon: 0.0,
            limit: 100,
            enforce: 1,
        }
    }

    #[test]
    fn start_then_end_makes_section() {
        let pair = |end, limit| SectionPair {
            end,
            length_m: 7_000.0,
            limit,
        };
        let table = EnforceTable::from_pairs(HashMap::from([
            (1, pair(2, 100)),
            (3, pair(4, 100)),
            (5, pair(6, 80)),
        ]));
        // 1→2는 순서대로 지나 구간이 되고, 3→4는 1→2와 같은 자리(차로별 카메라)라 빼며, 6은 5보다 앞에 있어 구간이 아니다
        let pois = vec![
            camera(1, 1_000.0),
            camera(3, 1_005.0),
            camera(6, 2_000.0),
            camera(2, 8_100.0),
            camera(4, 8_102.0),
            camera(5, 9_000.0),
        ];
        let s = table.sections(&pois);
        assert_eq!(
            s,
            vec![EnforceSection {
                from_m: 1_000.0,
                to_m: 8_100.0,
                limit: 100
            }]
        );
    }
}
