//! 주의운전 구간. 상습 결빙구간, 안개 잦은 구간처럼 시작점과 끝점이 있는 구간(section)과 중심점 반경의 보호구역(zone)을
//! 경로 좌표 열과 맞추어 경고 구간을 찾는다

pub mod construction;
pub mod disaster;
pub mod enforce;
pub mod incident;
pub mod notice;
pub mod rest;
pub mod risk;
pub mod section;
pub mod zone;

use serde_json::Value;

use crate::accident::index::cumulative_m;

/// 경로 좌표마다 출발점부터의 거리(m)
pub fn route_position(geometry: &[(f64, f64)], distance_m: f64) -> Vec<f64> {
    cumulative_m(geometry, distance_m)
}

/// 점 (lat, lon)에서 경로 좌표 열까지 가장 가까운 거리(m)와 그 위치의 출발점부터 거리(m). position은 route_position의 결과이다
pub fn nearest_on_route(
    geometry: &[(f64, f64)],
    position: &[f64],
    lat: f64,
    lon: f64,
) -> Option<(f64, f64)> {
    let cos = lat.to_radians().cos();
    let xy = |v: (f64, f64)| ((v.1 - lon) * 111_320.0 * cos, (v.0 - lat) * 110_950.0);
    let mut best: Option<(f64, f64)> = None;
    for (i, pair) in geometry.windows(2).enumerate() {
        let (a, b) = (xy(pair[0]), xy(pair[1]));
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 == 0.0 {
            0.0
        } else {
            (-(a.0 * dx + a.1 * dy) / len2).clamp(0.0, 1.0)
        };
        let d = (a.0 + t * dx).hypot(a.1 + t * dy);
        if best.is_none_or(|(old, _)| d < old) {
            best = Some((d, position[i] + t * (position[i + 1] - position[i])));
        }
    }
    best
}

/// 국가교통정보센터 응답의 header.resultCode가 0이 아니면 오류이다
pub fn result_code(body: &Value, name: &str) -> Result<(), String> {
    let code = match &body["header"]["resultCode"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => "0".to_string(),
    };
    if code == "0" {
        Ok(())
    } else {
        Err(format!(
            "{name} 오류 {code}: {}",
            body["header"]["resultMsg"].as_str().unwrap_or("")
        ))
    }
}
