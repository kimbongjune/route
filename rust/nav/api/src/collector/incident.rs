//! 국가교통정보센터 돌발상황정보 `/eventInfo` 폴링. 전면 통제는 돌발 배율 무한대, 부분 통제는 페널티 배율로 바꿔 적용 태스크에 보낸다.
//! 이전 폴링에 있던 링크가 이번에 없으면 배율 1.0으로 되돌린다

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use routedb_core::route::{FactorKind, RouteDb, WeightUpdate};
use serde::Deserialize;
use serde_json::Value;

use super::{Batch, Freshness, UpdateSender};

const EVENT_URL: &str = "https://openapi.its.go.kr:9443/eventInfo";
/// 부분 통제(차로 차단)의 돌발 배율
pub const PARTIAL_BLOCK_FACTOR: f32 = 2.0;
/// 전면 통제인 이벤트세부유형. 23 전면차단, 26 도로진입제한, 31 전면차단
const FULL_BLOCK_TYPES: [&str; 3] = ["23", "26", "31"];
/// 부분 통제인 이벤트세부유형. 30 차로차단
const PARTIAL_BLOCK_TYPES: [&str; 1] = ["30"];

/// `/eventInfo` 응답 한 건. 값은 모두 문자열이며 없는 항목은 빈 문자열이다
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct IncidentItem {
    #[serde(rename = "linkId", default)]
    pub link_id: String,
    #[serde(rename = "eventType", default)]
    pub event_type: String,
    #[serde(rename = "eventDetailType", default)]
    pub event_detail_type: String,
    #[serde(rename = "lanesBlockType", default)]
    pub lanes_block_type: String,
    #[serde(rename = "lanesBlocked", default)]
    pub lanes_blocked: String,
    #[serde(default)]
    pub message: String,
    #[serde(rename = "startDate", default)]
    pub start_date: String,
    #[serde(rename = "endDate", default)]
    pub end_date: String,
}

/// 통제 정도
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Block {
    /// 통행 가능. 배율 1.0
    None,
    /// 차로 일부 차단. PARTIAL_BLOCK_FACTOR
    Partial,
    /// 전면 차단. 배율 무한대
    Full,
}

impl Block {
    pub fn factor(self) -> f32 {
        match self {
            Block::None => 1.0,
            Block::Partial => PARTIAL_BLOCK_FACTOR,
            Block::Full => f32::INFINITY,
        }
    }
}

/// 폴링 한 번의 결과
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PollReport {
    /// 받은 이벤트 수
    pub items: usize,
    /// 링크 ID가 없는 이벤트 수
    pub no_link: usize,
    /// 그래프에 없는 LINK_ID 수
    pub unknown: usize,
    /// 전면·부분 통제 링크 수
    pub full: usize,
    pub partial: usize,
    /// 이전 폴링에 있다가 사라져 1.0으로 되돌린 링크 수
    pub cleared: usize,
}

/// 이벤트의 통제 정도. 이벤트세부유형 코드로 정하고, 코드에 없으면 차단 통제 유형 문구의 "전면"·"부분"과 차단 차로로 정한다
pub fn block_of(item: &IncidentItem) -> Block {
    let detail = item.event_detail_type.trim();
    if FULL_BLOCK_TYPES.contains(&detail) || item.lanes_block_type.contains("전면") {
        return Block::Full;
    }
    if PARTIAL_BLOCK_TYPES.contains(&detail)
        || item.lanes_block_type.contains("부분")
        || !item.lanes_blocked.trim().is_empty()
    {
        return Block::Partial;
    }
    Block::None
}

/// 응답 JSON의 `body.items`를 꺼낸다
pub fn parse_items(body: &Value) -> Result<Vec<IncidentItem>, String> {
    let code = match &body["header"]["resultCode"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => "0".to_string(),
    };
    if code != "0" {
        return Err(format!(
            "돌발정보 오류 {code}: {}",
            body["header"]["resultMsg"].as_str().unwrap_or("")
        ));
    }
    let items = body["body"]["items"].clone();
    if items.is_null() || items.as_str() == Some("") {
        return Ok(Vec::new());
    }
    serde_json::from_value(items).map_err(|e| format!("돌발정보 항목 해석 실패: {e}"))
}

/// 이벤트를 돌발 배율로 바꾼다. 링크마다 가장 센 통제를 쓰고, `previous`에 있다가 이번에 없는 링크는 1.0으로 되돌린다.
/// 반환값은 배율 묶음과 이번에 통제된 링크 집합이다
pub fn to_updates(
    items: &[IncidentItem],
    previous: &HashSet<u64>,
    db: &RouteDb,
    report: &mut PollReport,
) -> (Vec<WeightUpdate>, HashSet<u64>) {
    let mut strongest: std::collections::HashMap<u64, Block> = std::collections::HashMap::new();
    for item in items {
        let Ok(link_id) = item.link_id.trim().parse::<u64>() else {
            report.no_link += 1;
            continue;
        };
        if db.edge_of_link(link_id).is_none() {
            report.unknown += 1;
            continue;
        }
        let block = block_of(item);
        if block == Block::None {
            continue;
        }
        let entry = strongest.entry(link_id).or_insert(block);
        if block == Block::Full {
            *entry = Block::Full;
        }
    }
    let mut updates = Vec::with_capacity(strongest.len() + previous.len());
    let mut current = HashSet::with_capacity(strongest.len());
    for (&link_id, &block) in &strongest {
        match block {
            Block::Full => report.full += 1,
            Block::Partial => report.partial += 1,
            Block::None => {}
        }
        current.insert(link_id);
        updates.push(WeightUpdate {
            link_id,
            speed_factor: None,
            incident_factor: Some(block.factor()),
        });
    }
    for &link_id in previous.difference(&current) {
        report.cleared += 1;
        updates.push(WeightUpdate {
            link_id,
            speed_factor: None,
            incident_factor: Some(1.0),
        });
    }
    (updates, current)
}

/// 전국 돌발정보를 한 번 요청한다
pub async fn fetch_all(http: &reqwest::Client, key: &str) -> Result<Vec<IncidentItem>, String> {
    let params = [
        ("apiKey", key),
        ("type", "all"),
        ("eventType", "all"),
        ("getType", "json"),
    ];
    let body: Value = http
        .get(EVENT_URL)
        .query(&params)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("돌발정보 호출 실패: {e}"))?
        .json()
        .await
        .map_err(|e| format!("돌발정보 응답이 JSON이 아님: {e}"))?;
    parse_items(&body)
}

/// 돌발정보를 받아 배율로 바꿔 적용 태스크에 보낸다. 호출이 실패하면 이전 배율을 그대로 유지하고 None을 반환한다
pub async fn poll_once(
    http: &reqwest::Client,
    key: &str,
    db: &RouteDb,
    updates: &UpdateSender,
    previous: &HashSet<u64>,
) -> Option<(PollReport, HashSet<u64>)> {
    let started = Instant::now();
    let mut report = PollReport::default();
    let items = match fetch_all(http, key).await {
        Ok(items) => items,
        Err(message) => {
            eprintln!("돌발정보 폴링 실패: {message}");
            return None;
        }
    };
    report.items = items.len();
    let (batch, current) = to_updates(&items, previous, db, &mut report);
    if !batch.is_empty() && updates.send(Batch::Updates(batch)).await.is_err() {
        eprintln!("적용 태스크가 없어 돌발정보를 버림");
    }
    println!(
        "돌발정보 폴링 이벤트 {} 링크 없음 {} 없는 링크 {} 전면 {} 부분 {} 해제 {} ({:.1}초)",
        report.items,
        report.no_link,
        report.unknown,
        report.full,
        report.partial,
        report.cleared,
        started.elapsed().as_secs_f64()
    );
    Some((report, current))
}

/// 폴링 태스크. `interval`마다 `poll_once`를 실행하며 통제된 링크 집합을 이어서 갖고 있다.
/// 호출 실패가 STALE_POLLS 번 이어지면 돌발 배율을 되돌리고 통제 집합을 비운다
pub async fn run(
    http: reqwest::Client,
    key: String,
    db: Arc<RouteDb>,
    updates: UpdateSender,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    let mut previous = HashSet::new();
    let mut fresh = Freshness::new();
    loop {
        ticker.tick().await;
        let now = Instant::now();
        match poll_once(&http, &key, &db, &updates, &previous).await {
            Some((_, current)) => {
                previous = current;
                fresh.succeeded(now);
            }
            None if fresh.failed(now, interval) => {
                println!("돌발정보를 오래 받지 못해 돌발 배율을 되돌림");
                previous.clear();
                let _ = updates.send(Batch::Reset(FactorKind::Incident)).await;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn item(detail: &str, block_type: &str, lanes: &str) -> IncidentItem {
        IncidentItem {
            link_id: "3940006601".to_string(),
            event_detail_type: detail.to_string(),
            lanes_block_type: block_type.to_string(),
            lanes_blocked: lanes.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn block_levels() {
        assert_eq!(block_of(&item("31", "", "")), Block::Full);
        assert_eq!(block_of(&item("23", "", "")), Block::Full);
        assert_eq!(block_of(&item("26", "", "")), Block::Full);
        assert_eq!(block_of(&item("01", "전면차단", "")), Block::Full);
        assert_eq!(block_of(&item("30", "", "")), Block::Partial);
        assert_eq!(block_of(&item("00", "부분차단", "")), Block::Partial);
        assert_eq!(block_of(&item("03", "", "2")), Block::Partial);
        assert_eq!(block_of(&item("01", "", "")), Block::None);
        assert_eq!(Block::Partial.factor(), PARTIAL_BLOCK_FACTOR);
        assert!(Block::Full.factor().is_infinite());
    }

    #[test]
    fn parse_items_and_errors() {
        // 기술기준 문서의 샘플 한 건
        let body = json!({"header": {"resultCode": "0", "resultMsg": "SUCCESS"}, "body": {"totalCount": "1", "items": [
            {"type": "고속도로", "eventType": "기타돌발", "eventDetailType": "01", "startDate": "20260731140919", "coordX": "127.871905", "coordY": "35.03157",
             "linkId": "3940006601", "roadName": "남해선(순천-부산)", "roadNo": "10", "roadDrcType": "종점", "lanesBlockType": "", "lanesBlocked": "",
             "message": "(갓길)저속차량 주의", "endDate": ""}
        ]}});
        let items = parse_items(&body).expect("items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].link_id, "3940006601");
        assert_eq!(block_of(&items[0]), Block::None);
        let error = json!({"header": {"resultCode": 4005, "resultMsg": "인증키가 승인되지 않은 상태입니다."}, "body": ""});
        assert_eq!(
            parse_items(&error),
            Err("돌발정보 오류 4005: 인증키가 승인되지 않은 상태입니다.".to_string())
        );
        let empty = json!({"header": {"resultCode": "0"}, "body": ""});
        assert_eq!(parse_items(&empty).expect("빈 목록").len(), 0);
    }
}
