//! 사고다발 폴리곤 R-tree. 경로 좌표 열의 선분마다 R-tree로 후보 폴리곤을 찾고 선분과 폴리곤의 교차를 확인한다

use std::io;
use std::path::Path;

use geo::{Coord, Intersects, Line, LineString, Polygon};
use rstar::{AABB, RTree, RTreeObject};
use serde::Deserialize;

use super::model::{AccidentSection, AccidentZone};

/// R-tree에 넣는 폴리곤의 경계 상자와 번호
struct ZoneBox {
    index: usize,
    envelope: AABB<[f64; 2]>,
}

impl RTreeObject for ZoneBox {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

/// 사고다발_폴리곤.csv 한 행
#[derive(Deserialize)]
struct Row {
    #[serde(rename = "유형")]
    kind: String,
    id: String,
    #[serde(rename = "이름")]
    name: String,
    #[serde(rename = "사고건수")]
    accidents: u32,
    #[serde(rename = "사상자수")]
    casualties: u32,
    #[serde(rename = "사망자수")]
    deaths: u32,
    #[serde(rename = "경도")]
    lon: f64,
    #[serde(rename = "위도")]
    lat: f64,
    #[serde(rename = "폴리곤")]
    polygon: String,
}

/// 사고다발지역 폴리곤 색인. 좌표는 WGS84 위경도이며 x가 경도, y가 위도이다
pub struct AccidentIndex {
    zones: Vec<AccidentZone>,
    polygons: Vec<Polygon<f64>>,
    tree: RTree<ZoneBox>,
}

impl AccidentIndex {
    /// 사고다발_폴리곤.csv를 읽어 R-tree를 만든다
    pub fn load(path: &Path) -> io::Result<Self> {
        Self::load_all(&[path])
    }

    /// 같은 컬럼의 폴리곤 CSV 여러 개(사고다발, TAAS 사고다발, 도로환경 위험요소)를 읽어 한 색인으로 만든다
    pub fn load_all(paths: &[&Path]) -> io::Result<Self> {
        let mut zones = Vec::new();
        let mut polygons = Vec::new();
        for path in paths {
            Self::read(path, &mut zones, &mut polygons)?;
        }
        Ok(Self::build(zones, polygons))
    }

    fn read(
        path: &Path,
        zones: &mut Vec<AccidentZone>,
        polygons: &mut Vec<Polygon<f64>>,
    ) -> io::Result<()> {
        let mut reader = csv::Reader::from_path(path)?;
        for row in reader.deserialize::<Row>() {
            let row = row.map_err(io::Error::other)?;
            let ring = parse_ring(&row.polygon).map_err(io::Error::other)?;
            zones.push(AccidentZone {
                kind: row.kind,
                id: row.id,
                name: row.name,
                accidents: row.accidents,
                casualties: row.casualties,
                deaths: row.deaths,
                lat: row.lat,
                lon: row.lon,
            });
            polygons.push(ring);
        }
        Ok(())
    }

    /// 폴리곤 목록으로 색인을 만든다
    pub fn build(zones: Vec<AccidentZone>, polygons: Vec<Polygon<f64>>) -> Self {
        let boxes = polygons
            .iter()
            .enumerate()
            .map(|(index, polygon)| ZoneBox {
                index,
                envelope: bounding_box(polygon.exterior().coords()),
            })
            .collect();
        Self {
            zones,
            polygons,
            tree: RTree::bulk_load(boxes),
        }
    }

    pub fn len(&self) -> usize {
        self.zones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.zones.is_empty()
    }

    /// 경로 좌표 열 (위도, 경도)이 지나는 다발지역을 유형별 구간으로 반환한다.
    /// 거리는 좌표 열의 누적 길이를 경로 거리 `distance_m`에 비례하도록 맞춘 값이다
    pub fn sections(&self, geometry: &[(f64, f64)], distance_m: f64) -> Vec<AccidentSection> {
        let cumulative = cumulative_m(geometry, distance_m);
        // 폴리곤마다 처음 교차한 선분의 시작 거리와 마지막 교차한 선분의 끝 거리
        let mut spans: Vec<Option<(f64, f64)>> = vec![None; self.zones.len()];
        for (i, pair) in geometry.windows(2).enumerate() {
            let line = Line::new(
                Coord {
                    x: pair[0].1,
                    y: pair[0].0,
                },
                Coord {
                    x: pair[1].1,
                    y: pair[1].0,
                },
            );
            let envelope = AABB::from_corners(
                [line.start.x.min(line.end.x), line.start.y.min(line.end.y)],
                [line.start.x.max(line.end.x), line.start.y.max(line.end.y)],
            );
            for candidate in self.tree.locate_in_envelope_intersecting(envelope) {
                if !line.intersects(&self.polygons[candidate.index]) {
                    continue;
                }
                let span = &mut spans[candidate.index];
                let (from_m, to_m) = (cumulative[i], cumulative[i + 1]);
                *span = Some(match *span {
                    Some((first, _)) => (first, to_m),
                    None => (from_m, to_m),
                });
            }
        }
        let mut hits: Vec<(usize, f64, f64)> = spans
            .iter()
            .enumerate()
            .filter_map(|(index, span)| span.map(|(from_m, to_m)| (index, from_m, to_m)))
            .collect();
        hits.sort_by(|a, b| {
            self.zones[a.0]
                .kind
                .cmp(&self.zones[b.0].kind)
                .then(a.1.total_cmp(&b.1))
        });
        // 같은 유형에서 구간이 겹치거나 이어지면 한 구간으로 합친다
        let mut sections: Vec<AccidentSection> = Vec::new();
        for (index, from_m, to_m) in hits {
            let zone = &self.zones[index];
            if let Some(last) = sections.last_mut()
                && last.kind == zone.kind
                && from_m <= last.to_m
            {
                last.to_m = last.to_m.max(to_m);
                last.zones.push(zone.clone());
                continue;
            }
            sections.push(AccidentSection {
                kind: zone.kind.clone(),
                from_m,
                to_m,
                zones: vec![zone.clone()],
            });
        }
        sections.sort_by(|a, b| a.from_m.total_cmp(&b.from_m));
        sections
    }
}

/// "경도 위도,경도 위도,..." 문자열을 폴리곤으로 바꾼다
pub fn parse_ring(text: &str) -> Result<Polygon<f64>, String> {
    let mut coords = Vec::new();
    for pair in text.split(',') {
        let mut parts = pair.split_whitespace();
        let (Some(x), Some(y)) = (parts.next(), parts.next()) else {
            return Err(format!("폴리곤 점이 잘못됨: {pair}"));
        };
        let x: f64 = x.parse().map_err(|_| format!("경도가 숫자가 아님: {x}"))?;
        let y: f64 = y.parse().map_err(|_| format!("위도가 숫자가 아님: {y}"))?;
        coords.push(Coord { x, y });
    }
    if coords.len() < 3 {
        return Err("폴리곤 점이 3개 미만".to_string());
    }
    Ok(Polygon::new(LineString::new(coords), Vec::new()))
}

fn bounding_box<'a>(coords: impl Iterator<Item = &'a Coord<f64>>) -> AABB<[f64; 2]> {
    let mut min = [f64::MAX; 2];
    let mut max = [f64::MIN; 2];
    for c in coords {
        min = [min[0].min(c.x), min[1].min(c.y)];
        max = [max[0].max(c.x), max[1].max(c.y)];
    }
    AABB::from_corners(min, max)
}

/// 좌표 열의 누적 거리. 위경도 차이를 미터로 바꾼 길이를 합하고 경로 거리에 비례하도록 맞춘다
pub(crate) fn cumulative_m(geometry: &[(f64, f64)], distance_m: f64) -> Vec<f64> {
    let mut cumulative = Vec::with_capacity(geometry.len());
    let mut total = 0.0;
    cumulative.push(0.0);
    for pair in geometry.windows(2) {
        let lat = ((pair[0].0 + pair[1].0) / 2.0).to_radians();
        let dx = (pair[1].1 - pair[0].1) * 111_320.0 * lat.cos();
        let dy = (pair[1].0 - pair[0].0) * 110_950.0;
        total += (dx * dx + dy * dy).sqrt();
        cumulative.push(total);
    }
    if total > 0.0 {
        let scale = distance_m / total;
        for c in &mut cumulative {
            *c *= scale;
        }
    }
    cumulative
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(kind: &str, id: &str) -> AccidentZone {
        AccidentZone {
            kind: kind.to_string(),
            id: id.to_string(),
            name: id.to_string(),
            accidents: 3,
            casualties: 3,
            deaths: 0,
            lat: 37.0,
            lon: 127.0,
        }
    }

    #[test]
    fn sections_along_route() {
        // 경도 127.000~127.001의 정사각형 두 개가 겹치고(보행자), 127.003~127.004에 하나(자전거), 127.010에 하나는 멀리 있다
        let square = |x0: f64, x1: f64| {
            parse_ring(&format!(
                "{x0} 37.000,{x1} 37.000,{x1} 37.001,{x0} 37.001,{x0} 37.000"
            ))
            .expect("폴리곤")
        };
        let index = AccidentIndex::build(
            vec![
                zone("보행자", "a"),
                zone("보행자", "b"),
                zone("자전거", "c"),
                zone("자전거", "d"),
            ],
            vec![
                square(127.000, 127.001),
                square(127.0005, 127.0015),
                square(127.003, 127.004),
                square(127.010, 127.011),
            ],
        );
        assert_eq!(index.len(), 4);
        // 위도 37.0005를 따라 동쪽으로 가는 경로. 좌표 간격이 0.0005도이며 전체 길이를 1,000m로 맞춘다
        let geometry: Vec<(f64, f64)> = (0..=10)
            .map(|k| (37.0005, 126.9995 + k as f64 * 0.0005))
            .collect();
        let sections = index.sections(&geometry, 1000.0);
        let kinds: Vec<(&str, usize)> = sections
            .iter()
            .map(|s| (s.kind.as_str(), s.zones.len()))
            .collect();
        // 보행자 두 폴리곤은 겹쳐서 한 구간, 자전거는 한 구간이고 먼 폴리곤은 없다
        assert_eq!(kinds, [("보행자", 2), ("자전거", 1)]);
        // 선분 끝점이 폴리곤 경계에 닿는 것도 교차이므로 보행자 구간은 첫 선분(0~100m)부터
        // 경계 127.0015에 닿는 다섯 번째 선분 끝(500m)까지이고, 자전거 구간은 600~1,000m이다
        assert_eq!(
            (sections[0].from_m.round(), sections[0].to_m.round()),
            (0.0, 500.0)
        );
        assert_eq!(
            (sections[1].from_m.round(), sections[1].to_m.round()),
            (600.0, 1000.0)
        );
        // 폴리곤 밖을 지나는 경로는 구간이 없다
        let outside = [(37.005, 126.999), (37.005, 127.012)];
        assert!(index.sections(&outside, 1000.0).is_empty());
    }

    #[test]
    fn parse_ring_rejects_bad_text() {
        assert!(parse_ring("127.0 37.0,127.1 37.0").is_err());
        assert!(parse_ring("127.0 37.0,abc 37.0,127.1 37.1").is_err());
        assert!(parse_ring("127.0 37.0,127.1 37.0,127.1 37.1").is_ok());
    }
}
