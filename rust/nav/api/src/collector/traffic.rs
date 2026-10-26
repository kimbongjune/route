//! 국가교통정보센터 교통소통정보 `/trafficInfo` 폴링. 링크별 통행 속도를 속도 배율로 바꿔 적용 태스크에 보낸다.
//! 전국을 한 번에 요청하면 응답이 오지 않으므로 1.5도 타일로 나누어 요청한다

use std::sync::Arc;
use std::time::{Duration, Instant};

use routedb_core::route::{FactorKind, RouteDb, WeightUpdate};
use serde::Deserialize;
use serde_json::Value;

use super::{Batch, Freshness, UpdateSender};

const TRAFFIC_URL: &str = "https://openapi.its.go.kr:9443/trafficInfo";
/// 타일 한 변(도). 2도 타일은 응답이 오지 않았고 1.5도 타일은 7초 안에 왔다
const TILE_DEG: f64 = 1.5;
/// 전국 범위. 경도 125~131, 위도 33~39
const LON_RANGE: (f64, f64) = (125.0, 131.0);
const LAT_RANGE: (f64, f64) = (33.0, 39.0);
/// 속도 배율 상한. 제한속도의 1/10 아래 속도는 상한으로 본다
const MAX_SPEED_FACTOR: f32 = 10.0;

/// `/trafficInfo` 응답 한 건. 값은 모두 문자열이다
#[derive(Debug, Deserialize, PartialEq)]
pub struct TrafficItem {
    #[serde(rename = "linkId")]
    pub link_id: String,
    /// 통행 속도(km/h)
    pub speed: String,
    #[serde(rename = "travelTime")]
    pub travel_time: String,
    #[serde(rename = "createdDate")]
    pub created_date: String,
}

/// 폴링 한 번의 결과
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PollReport {
    /// 받은 타일 수와 실패한 타일 수
    pub tiles: usize,
    pub failed_tiles: usize,
    /// 받은 링크 수
    pub items: usize,
    /// 속도가 0이거나 숫자가 아니라 뺀 수
    pub skipped: usize,
    /// 그래프에 없는 LINK_ID 수
    pub unknown: usize,
    /// 만든 배율 수
    pub updates: usize,
}

/// 전국을 덮는 타일 (minX, maxX, minY, maxY)
pub fn tiles() -> Vec<(f64, f64, f64, f64)> {
    let mut out = Vec::new();
    let mut lon = LON_RANGE.0;
    while lon < LON_RANGE.1 {
        let mut lat = LAT_RANGE.0;
        while lat < LAT_RANGE.1 {
            out.push((lon, lon + TILE_DEG, lat, lat + TILE_DEG));
            lat += TILE_DEG;
        }
        lon += TILE_DEG;
    }
    out
}

/// 응답 JSON의 `body.items`를 꺼낸다. `resultCode`가 0이 아니면 메시지를 에러로 돌려준다
pub fn parse_items(body: &Value) -> Result<Vec<TrafficItem>, String> {
    let code = body["header"]["resultCode"]
        .as_str()
        .or_else(|| body["header"]["resultCode"].as_i64().map(|_| "0"))
        .unwrap_or("0");
    if code != "0" {
        return Err(format!(
            "소통정보 오류 {code}: {}",
            body["header"]["resultMsg"].as_str().unwrap_or("")
        ));
    }
    let items = body["body"]["items"].clone();
    if items.is_null() {
        return Ok(Vec::new());
    }
    serde_json::from_value(items).map_err(|e| format!("소통정보 항목 해석 실패: {e}"))
}

/// 링크별 속도를 속도 배율로 바꾼다. 배율은 제한속도 ÷ 통행 속도이며 1.0 아래로는 내리지 않고 상한은 MAX_SPEED_FACTOR이다.
/// 속도가 0이거나 숫자가 아니면 빼고, 같은 링크가 여러 번 오면 마지막 값이다
pub fn to_updates(
    items: &[TrafficItem],
    db: &RouteDb,
    report: &mut PollReport,
) -> Vec<WeightUpdate> {
    let max_spd = db.file.max_spd();
    let mut updates: Vec<WeightUpdate> = Vec::with_capacity(items.len());
    let mut index_of: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for item in items {
        let (Ok(link_id), Ok(speed)) = (
            item.link_id.parse::<u64>(),
            item.speed.trim().parse::<f32>(),
        ) else {
            report.skipped += 1;
            continue;
        };
        if speed <= 0.0 {
            report.skipped += 1;
            continue;
        }
        let Some(e) = db.edge_of_link(link_id) else {
            report.unknown += 1;
            continue;
        };
        let factor = (max_spd[e as usize] as f32 / speed).clamp(1.0, MAX_SPEED_FACTOR);
        let update = WeightUpdate {
            link_id,
            speed_factor: Some(factor),
            incident_factor: None,
        };
        match index_of.get(&link_id) {
            Some(&i) => updates[i] = update,
            None => {
                index_of.insert(link_id, updates.len());
                updates.push(update);
            }
        }
    }
    report.updates = updates.len();
    updates
}

/// 타일 하나를 요청한다
pub async fn fetch_tile(
    http: &reqwest::Client,
    key: &str,
    tile: (f64, f64, f64, f64),
) -> Result<Vec<TrafficItem>, String> {
    let params = [
        ("apiKey", key.to_string()),
        ("type", "all".to_string()),
        ("minX", format!("{:.3}", tile.0)),
        ("maxX", format!("{:.3}", tile.1)),
        ("minY", format!("{:.3}", tile.2)),
        ("maxY", format!("{:.3}", tile.3)),
        ("getType", "json".to_string()),
    ];
    let body: Value = http
        .get(TRAFFIC_URL)
        .query(&params)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("소통정보 호출 실패: {e}"))?
        .json()
        .await
        .map_err(|e| format!("소통정보 응답이 JSON이 아님: {e}"))?;
    parse_items(&body)
}

/// 전국 타일을 차례로 받아 배율로 바꿔 적용 태스크에 보낸다. 실패한 타일은 건너뛰고 그 링크들은 이전 배율을 유지한다
pub async fn poll_once(
    http: &reqwest::Client,
    key: &str,
    db: &RouteDb,
    updates: &UpdateSender,
) -> PollReport {
    let started = Instant::now();
    let mut report = PollReport::default();
    let mut items = Vec::new();
    for tile in tiles() {
        report.tiles += 1;
        match fetch_tile(http, key, tile).await {
            Ok(mut got) => items.append(&mut got),
            Err(message) => {
                report.failed_tiles += 1;
                eprintln!("소통정보 타일 {tile:?} 실패: {message}");
            }
        }
    }
    report.items = items.len();
    let batch = to_updates(&items, db, &mut report);
    if !batch.is_empty() && updates.send(Batch::Updates(batch)).await.is_err() {
        eprintln!("적용 태스크가 없어 소통정보를 버림");
    }
    println!(
        "소통정보 폴링 타일 {} 실패 {} 링크 {} 제외 {} 없는 링크 {} 배율 {} ({:.1}초)",
        report.tiles,
        report.failed_tiles,
        report.items,
        report.skipped,
        report.unknown,
        report.updates,
        started.elapsed().as_secs_f64()
    );
    report
}

/// 폴링 태스크. `interval`마다 `poll_once`를 실행한다. 타일이 모두 실패했거나 받은 링크가 0개인 폴링이 STALE_POLLS 번 이어지면
/// 속도 배율을 되돌린다. 국가교통정보센터가 성공 코드와 빈 목록을 주는 때가 있어(2026-10-26 19시 50분) 0개는 실패로 센다
pub async fn run(
    http: reqwest::Client,
    key: String,
    db: Arc<RouteDb>,
    updates: UpdateSender,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    let mut fresh = Freshness::new();
    loop {
        ticker.tick().await;
        let report = poll_once(&http, &key, &db, &updates).await;
        let now = Instant::now();
        if report.failed_tiles < report.tiles && report.items > 0 {
            fresh.succeeded(now);
        } else if fresh.failed(now, interval) {
            println!("소통정보를 오래 받지 못해 속도 배율을 되돌림");
            let _ = updates.send(Batch::Reset(FactorKind::Speed)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tiles_cover_korea() {
        let t = tiles();
        assert_eq!(t.len(), 16);
        assert_eq!(t[0], (125.0, 126.5, 33.0, 34.5));
        assert_eq!(t[15], (129.5, 131.0, 37.5, 39.0));
    }

    #[test]
    fn parse_items_and_errors() {
        let body = json!({"header": {"resultCode": "0", "resultMsg": "SUCCESS"}, "body": {"totalCount": "2", "items": [
            {"roadName": "청계천로", "roadDrcType": "", "linkNo": "", "linkId": "1000002701", "startNodeId": "1000006600", "endNodeId": "1010032700", "speed": "12", "travelTime": "49.89", "createdDate": "20261024003500"},
            {"roadName": "청계천로", "roadDrcType": "", "linkNo": "", "linkId": "1000002702", "startNodeId": "1000001600", "endNodeId": "1000006600", "speed": "36", "travelTime": "24.73", "createdDate": "20261024003500"}
        ]}});
        let items = parse_items(&body).expect("items");
        assert_eq!(items.len(), 2);
        assert_eq!(
            (items[0].link_id.as_str(), items[0].speed.as_str()),
            ("1000002701", "12")
        );
        let error = json!({"header": {"resultCode": "4001", "resultMsg": "사용량 제한"}});
        assert_eq!(
            parse_items(&error),
            Err("소통정보 오류 4001: 사용량 제한".to_string())
        );
        let empty = json!({"header": {"resultCode": "0"}, "body": {"items": null}});
        assert_eq!(parse_items(&empty).expect("빈 목록").len(), 0);
    }
}
