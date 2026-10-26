//! 주의운전구간 실시간 메시지(국가교통정보센터 `/posIncidentInfo`). 한국도로공사가 1분 주기로 주는 고속도로 사고·작업·정체·사고잦은구간·졸음쉼터 등의
//! 제공 문안을 주기적으로 받아 두고, 경로가 이벤트 시작 링크를 지나면 경고한다. 받은 지 NOTICE_MAX_AGE가 지난 자료는 쓰지 않는다

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use super::{nearest_on_route, result_code, route_position};

const NOTICE_URL: &str = "https://openapi.its.go.kr:9443/posIncidentInfo";
/// 전국을 덮는 요청 영역 (minX, maxX, minY, maxY)
const NATION: (f64, f64, f64, f64) = (124.0, 132.0, 33.0, 39.0);
/// 시작 링크가 없는 이벤트는 시작 지점이 경로에서 이 거리 안이면 경고한다(m)
pub const NOTICE_MATCH_M: f64 = 50.0;
/// 시작 링크가 경로에 있어도 시작 지점이 경로에서 이 거리보다 멀면 쓰지 않는다(m)
pub const NOTICE_LINK_M: f64 = 300.0;
/// 이보다 오래된 자료는 쓰지 않는다
pub const NOTICE_MAX_AGE: Duration = Duration::from_secs(30 * 60);

/// 응답 항목 하나. 값은 모두 문자열이다
#[derive(Debug, Default, Deserialize)]
struct Item {
    #[serde(default)]
    message: String,
    #[serde(rename = "stepType", default)]
    step: String,
    #[serde(rename = "outbrkType", default)]
    kind: String,
    #[serde(default)]
    priority: String,
    #[serde(rename = "startStdLinkId", default)]
    link: String,
    #[serde(rename = "startX", default)]
    x: String,
    #[serde(rename = "startY", default)]
    y: String,
    #[serde(rename = "occrrncId", default)]
    id: String,
}

/// 이벤트 하나. 같은 이벤트의 여러 문안 중 제공 순번과 우선순위가 가장 앞선 문안이다
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    /// 콘텐츠유형. 예: 사고, 작업, 사고잦은구간, 졸음쉼터
    pub kind: String,
    pub message: String,
    /// 이벤트 시작 링크 LINK_ID. 없으면 0
    pub link: u64,
    pub lat: f64,
    pub lon: f64,
}

/// 경로가 지나는 이벤트
#[derive(Clone, Debug, PartialEq)]
pub struct NoticeHit {
    pub at_m: f64,
    pub notice: Notice,
}

struct Snapshot {
    notices: Vec<Notice>,
    fetched: Instant,
}

/// 마지막으로 받은 이벤트. 폴링 태스크가 바꾸고 경로 응답이 읽는다
pub struct NoticeStore {
    snapshot: RwLock<Option<Snapshot>>,
}

impl Default for NoticeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NoticeStore {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(None),
        }
    }

    pub fn replace(&self, notices: Vec<Notice>) {
        *self.snapshot.write().expect("주의운전 잠금 실패") = Some(Snapshot {
            notices,
            fetched: Instant::now(),
        });
    }

    /// 경로가 시작 링크를 지나거나, 시작 링크가 없으면 시작 지점에서 NOTICE_MATCH_M 안을 지나는 이벤트. 거리 순서이다
    pub fn hits(&self, links: &[u64], geometry: &[(f64, f64)], distance_m: f64) -> Vec<NoticeHit> {
        let guard = self.snapshot.read().expect("주의운전 잠금 실패");
        let Some(snap) = guard
            .as_ref()
            .filter(|s| s.fetched.elapsed() <= NOTICE_MAX_AGE)
        else {
            return Vec::new();
        };
        let on_route: HashSet<u64> = links.iter().copied().collect();
        let position = route_position(geometry, distance_m);
        let mut out: Vec<NoticeHit> = snap
            .notices
            .iter()
            .filter_map(|n| {
                let limit = if n.link == 0 {
                    NOTICE_MATCH_M
                } else if on_route.contains(&n.link) {
                    NOTICE_LINK_M
                } else {
                    return None;
                };
                let (d, at_m) = nearest_on_route(geometry, &position, n.lat, n.lon)?;
                (d <= limit).then(|| NoticeHit {
                    at_m,
                    notice: n.clone(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
        out
    }
}

/// 응답 본문을 이벤트 목록으로 바꾼다. 이벤트ID가 같은 문안은 하나로 묶고, 이벤트ID가 없으면 유형·시작 링크·시작 좌표가 같은 문안을 묶는다
pub fn parse(body: &Value) -> Result<Vec<Notice>, String> {
    result_code(body, "주의운전")?;
    let items = body["body"]["items"].clone();
    if items.is_null() || items.as_str() == Some("") {
        return Ok(Vec::new());
    }
    let items: Vec<Item> =
        serde_json::from_value(items).map_err(|e| format!("주의운전 항목 해석 실패: {e}"))?;
    let mut groups: HashMap<String, ((u32, u32), Notice)> = HashMap::new();
    for i in items {
        let (Ok(lon), Ok(lat)) = (i.x.trim().parse::<f64>(), i.y.trim().parse::<f64>()) else {
            continue;
        };
        if lat == 0.0 || lon == 0.0 {
            continue;
        }
        let message = i.message.split_whitespace().collect::<Vec<_>>().join(" ");
        if message.is_empty() {
            continue;
        }
        let key = if i.id.trim().is_empty() {
            format!("{}|{}|{}|{}", i.kind, i.link, i.x, i.y)
        } else {
            i.id.trim().to_string()
        };
        let order = (
            i.step.trim().parse().unwrap_or(u32::MAX),
            i.priority.trim().parse().unwrap_or(u32::MAX),
        );
        if groups.get(&key).is_some_and(|(old, _)| *old <= order) {
            continue;
        }
        let notice = Notice {
            kind: i.kind.trim().to_string(),
            message,
            link: i.link.trim().parse().unwrap_or(0),
            lat,
            lon,
        };
        groups.insert(key, (order, notice));
    }
    let mut out: Vec<Notice> = groups.into_values().map(|(_, n)| n).collect();
    out.sort_by(|a, b| (a.link, &a.message).cmp(&(b.link, &b.message)));
    Ok(out)
}

/// interval마다 주의운전구간 메시지를 받아 store를 바꾼다. 실패하면 이전 자료를 그대로 둔다
pub async fn run(http: reqwest::Client, key: String, store: Arc<NoticeStore>, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let started = Instant::now();
        let result = async {
            let response = http
                .get(NOTICE_URL)
                .query(&[
                    ("apiKey", key.clone()),
                    ("minX", NATION.0.to_string()),
                    ("maxX", NATION.1.to_string()),
                    ("minY", NATION.2.to_string()),
                    ("maxY", NATION.3.to_string()),
                    ("getType", "json".to_string()),
                ])
                .timeout(Duration::from_secs(60))
                .send()
                .await
                .map_err(|e| format!("호출 실패: {}", e.without_url()))?;
            let body: Value = response
                .json()
                .await
                .map_err(|e| format!("본문 읽기 실패: {}", e.without_url()))?;
            parse(&body)
        }
        .await;
        match result {
            Ok(notices) => {
                println!(
                    "주의운전구간 이벤트 {}건 ({:.1}초)",
                    notices.len(),
                    started.elapsed().as_secs_f64()
                );
                store.replace(notices);
            }
            Err(e) => eprintln!("주의운전구간 실패: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_match() {
        let body: Value = serde_json::from_str(
            r#"{"header":{"resultCode":0,"resultMsg":"SUCCESS"},"body":{"totalCount":"4","items":[
            {"message":"보은부근 사고발생으로2km정체","stepType":"2","outbrkType":"사고","priority":"1","startStdLinkId":"100","startX":"127.0045","startY":"37.5003","occrrncId":"E1"},
            {"message":"전방 사고  추돌주의","stepType":"0","outbrkType":"사고","priority":"1","startStdLinkId":"100","startX":"127.0045","startY":"37.5003","occrrncId":"E1"},
            {"message":"사고잦은구간 추돌주의","stepType":"0","outbrkType":"사고잦은구간","priority":"1","startStdLinkId":"200","startX":"127.0045","startY":"37.5003","occrrncId":""},
            {"message":"졸음쉼터","stepType":"0","outbrkType":"졸음쉼터","priority":"1","startStdLinkId":"","startX":"127.0080","startY":"37.5001","occrrncId":""}]}}"#,
        )
        .unwrap();
        let notices = parse(&body).unwrap();
        assert_eq!(notices.len(), 3);
        assert!(notices.iter().any(|n| n.message == "전방 사고 추돌주의"));
        assert!(parse(&serde_json::json!({"header":{"resultCode":4002,"resultMsg":"x"}})).is_err());
        let store = NoticeStore::new();
        store.replace(notices);
        // 경로 링크 100만 지난다. 링크 200의 사고잦은구간은 빠지고 링크가 없는 졸음쉼터는 북쪽 11m라 들어간다
        let geometry = [(37.5, 127.0), (37.5, 127.01)];
        let hits = store.hits(&[100, 300], &geometry, 880.0);
        let messages: Vec<&str> = hits.iter().map(|h| h.notice.message.as_str()).collect();
        assert_eq!(messages, vec!["전방 사고 추돌주의", "졸음쉼터"]);
        assert!((hits[0].at_m - 396.0).abs() < 5.0, "{}", hits[0].at_m);
    }
}
