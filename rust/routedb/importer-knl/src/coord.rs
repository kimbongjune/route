use proj4rs::proj::Proj;

const EPSG_5186: &str =
    "+proj=tmerc +lat_0=38 +lon_0=127 +k=1 +x_0=200000 +y_0=600000 +ellps=GRS80 +units=m +no_defs";
const EPSG_4326: &str = "+proj=longlat +datum=WGS84 +no_defs";

/// 표준노드링크 TM 좌표(EPSG:5186)를 위경도(WGS84)로 변경
pub struct TmToWgs84 {
    from: Proj,
    to: Proj,
}

impl TmToWgs84 {
    pub fn new() -> Self {
        Self {
            from: Proj::from_proj_string(EPSG_5186).expect("EPSG:5186 정의가 잘못됨"),
            to: Proj::from_proj_string(EPSG_4326).expect("EPSG:4326 정의가 잘못됨"),
        }
    }

    /// (x, y)를 (위도, 경도)로 변환한다. proj4rs는 라디안으로 돌려주므로 도로 재 변환한다.
    pub fn convert(&self, x: f64, y: f64) -> (f64, f64) {
        let mut p = (x, y, 0.0);
        proj4rs::transform::transform(&self.from, &self.to, &mut p).expect("좌표 변환 실패");
        (p.1.to_degrees(), p.0.to_degrees())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_first_node_to_wgs84() {
        // 첫 노드 2290046502 능안교(동측)의 TM 좌표
        let (lat, lon) = TmToWgs84::new().convert(181789.12747089585, 571379.4025500367);
        assert!((lat - 37.74196185).abs() < 1e-7, "위도 {lat}");
        assert!((lon - 126.79338550).abs() < 1e-7, "경도 {lon}");
    }
}
