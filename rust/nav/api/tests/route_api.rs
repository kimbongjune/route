//! API 통합 테스트. 서버를 띄우지 않고 라우터에 HTTP 요청을 직접 보낸다.
//! 전국 그래프 파일을 열므로 maps/routedb가 있어야 한다.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use nav_api::accident::index::AccidentIndex;
use nav_api::collector::{self, Batch};
use nav_api::guideline::GuideLineTable;
use nav_api::route::toll::TollTable;
use nav_api::state::AppState;
use routedb_core::route::{RouteDb, WeightUpdate};
use routedb_geocode::GeocodeIndex;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tower::ServiceExt;

const GRAPH_DIR: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb";
const GUIDE_LINE_FILE: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\유도선\유도선_노드.csv";
const ACCIDENT_FILE: &str =
    r"C:\Users\nocdu\Desktop\dev\route\maps\전국교통사고다발지역표준데이터\사고다발_폴리곤.csv";
const TOLL_DIR: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\유료도로";

fn app() -> Router {
    app_with_db().0
}

/// 라우터와 함께 `RouteDb`도 반환한다. 가중치 갱신 테스트가 사용한다
fn app_with_db() -> (Router, Arc<RouteDb>) {
    let (app, db, _) = app_with_state();
    (app, db)
}

/// 라우터, `RouteDb`, 앱 상태를 반환한다. 적용 태스크와 이벤트 채널도 만든다
fn app_with_state() -> (Router, Arc<RouteDb>, AppState) {
    let dir = Path::new(GRAPH_DIR);
    let db = RouteDb::open(
        &dir.join("moct.rdb"),
        &dir.join("moct.cch-struct"),
        [
            &dir.join("moct.cch-metric"),
            &dir.join("moct.cch-metric-time"),
            &dir.join("moct.cch-metric-free"),
        ],
    )
    .expect("그래프 파일을 열지 못함");
    let accidents =
        AccidentIndex::load(Path::new(ACCIDENT_FILE)).expect("사고다발 폴리곤 파일을 읽지 못함");
    let db = Arc::new(db);
    let (events, _) = broadcast::channel(64);
    let updates = collector::start(Arc::clone(&db), events.clone());
    let state = AppState {
        db: Arc::clone(&db),
        accidents: Arc::new(accidents),
        guide_lines: Arc::new(
            GuideLineTable::load(Path::new(GUIDE_LINE_FILE)).expect("유도선 파일을 읽지 못함"),
        ),
        geocoder: Arc::new(
            GeocodeIndex::open(&dir.join("geocode.idx")).expect("지오코더 색인을 열지 못함"),
        ),
        tolls: Arc::new(
            TollTable::load(
                &Path::new(TOLL_DIR).join("영업소.csv"),
                &Path::new(TOLL_DIR).join("요금표.csv"),
            )
            .expect("통행료 표를 읽지 못함"),
        ),
        http: reqwest::Client::new(),
        vworld_key: String::new(),
        geoserver_url: String::new(),
        updates,
        events,
    };
    let app = nav_api::app(state.clone());
    (app, db, state)
}

/// 요청 하나를 보내고 (상태 코드, 본문)을 반환한다
async fn send(app: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("요청 생성 실패");
    let response = app.clone().oneshot(request).await.expect("요청 실패");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("본문 읽기 실패")
        .to_bytes();
    // axum이 본문 해석에 실패하면 JSON이 아닌 문자열을 반환하므로 그때는 문자열 값으로 돌려준다
    let json = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, json)
}

/// 두 방위각의 차이(0~180도)
fn heading_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

#[tokio::test]
async fn heading_picks_start_direction() {
    let app = app();
    // 서울역 앞 한강대로는 중앙분리 도로라 남쪽·북쪽 방위각에 따라 출발 간선이 다르다
    let mut first_links = Vec::new();
    for heading in [180.0, 0.0] {
        let (status, body) = send(
            &app,
            "POST",
            "/route",
            json!({"from": {"lat": 37.5547, "lon": 126.9707, "heading": heading}, "to": {"lat": 37.4979, "lon": 127.0276}}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let data = &body["data"];
        let bearing = data["from"]["bearing_deg"].as_f64().expect("bearing_deg");
        assert!(
            heading_diff(bearing, heading) <= 90.0,
            "방위각 {heading}에 출발 간선 방위각 {bearing}"
        );
        first_links.push(data["links"][0].as_u64().expect("links[0]"));
    }
    assert_ne!(first_links[0], first_links[1]);

    // 방위각 범위 밖은 400
    let (status, body) = send(
        &app,
        "POST",
        "/route",
        json!({"from": {"lat": 37.5547, "lon": 126.9707, "heading": 400.0}, "to": {"lat": 37.4979, "lon": 127.0276}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "방위각 범위를 벗어남: 400");
}

#[tokio::test]
async fn route_requests() {
    let app = app();

    // 서울역→강남역 거리 경로
    let (status, body) = send(
        &app,
        "POST",
        "/route",
        json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 37.4979, "lon": 127.0276}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], 200);
    assert_eq!(body["code"], "OK");
    let data = &body["data"];
    assert_eq!(data["profile"], "distance");
    assert_eq!(data["links"].as_array().map(Vec::len), Some(104));
    assert_eq!(data["steps"].as_array().map(Vec::len), Some(7));
    assert_eq!(data["steps"][0]["turn"], "left");
    assert_eq!(data["steps"][0]["to_name"], "한강대로77길");
    // 안내문은 안내 지점과 같은 수이고 첫 안내문은 남영역앞교차로 좌회전이다
    let instructions = data["instructions"].as_array().expect("instructions 배열");
    assert_eq!(instructions.len(), 7);
    assert_eq!(
        instructions[0]["text"],
        "남영역앞교차로에서 좌회전, 한강대로77길"
    );
    // 경고는 높이 제한이 맨 앞이고 거리 순서이다
    let warnings = data["warnings"].as_array().expect("warnings 배열");
    assert_eq!(warnings[0]["kind"], "height_limit");
    assert_eq!(warnings[0]["text"], "높이 제한 3.8m");
    let along: Vec<f64> = warnings.iter().filter_map(|w| w["at_m"].as_f64()).collect();
    assert!(along.windows(2).all(|p| p[0] <= p[1]));
    assert!(warnings.iter().any(|w| w["kind"] == "speed_camera"));
    assert!(warnings.iter().any(|w| w["kind"] == "accident_zone"));
    let sections = data["accident_sections"]
        .as_array()
        .expect("accident_sections 배열");
    assert!(!sections.is_empty());
    for s in sections {
        assert!(s["from_m"].as_f64().expect("from_m") <= s["to_m"].as_f64().expect("to_m"));
        assert!(!s["zones"].as_array().expect("zones").is_empty());
    }
    let summary: Vec<Value> = sections
        .iter()
        .map(|s| json!({"kind": s["kind"], "from_m": s["from_m"], "to_m": s["to_m"], "zones": s["zones"].as_array().map(Vec::len)}))
        .collect();
    eprintln!("사고다발 구간 {}", Value::Array(summary));

    // 서울역→부산역 최소시간 경로는 영업소 4곳을 지나고 통행료가 있다
    let (status, body) = send(
        &app,
        "POST",
        "/route",
        json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 35.1151, "lon": 129.0422}, "profile": "time"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["tolls"].as_array().map(Vec::len), Some(4));
    let toll = &body["data"]["toll"];
    eprintln!("통행료 {}", toll);
    assert!(toll["won"].as_u64().unwrap_or(0) > 0, "{toll}");
    assert!(
        body["data"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["kind"] == "toll" && w["at_m"].is_number())
    );

    // 후보 비교. 세 프로파일 중 같은 경로는 묶이고 대안이 더해지며 가장 빠른 후보와 최소 요금 후보가 표시된다
    let (routes_status, routes_body) = send(
        &app,
        "POST",
        "/routes",
        json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 35.1151, "lon": 129.0422}}),
    )
    .await;
    assert_eq!(routes_status, StatusCode::OK);
    let candidates = routes_body["data"]["candidates"]
        .as_array()
        .expect("candidates");
    // 프로파일 경로 최대 3개에 최소시간 경로의 대안 최대 2개
    assert!((1..=5).contains(&candidates.len()), "{}", candidates.len());
    assert_eq!(
        candidates
            .iter()
            .map(|c| c["profiles"].as_array().unwrap().len())
            .sum::<usize>(),
        3
    );
    assert!(candidates.iter().any(|c| c["fastest"] == true));
    assert!(candidates.iter().any(|c| c["cheapest"] == true));
    // 고속도로 출구 안내에 유도선 색이 붙는다
    let texts: Vec<&str> = body["data"]["instructions"]
        .as_array()
        .expect("instructions")
        .iter()
        .filter_map(|i| i["text"].as_str())
        .collect();
    assert!(
        texts.contains(&"신갈JC(북측)에서 오른쪽 출구, 영동고속도로 방면, 분홍색 유도선"),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"반포IC에서 오른쪽 출구, 경부고속도로 방면"),
        "{texts:?}"
    );

    // 바다 위 출발점은 경로가 없다
    let (status, body) = send(
        &app,
        "POST",
        "/route",
        json!({"from": {"lat": 33.0, "lon": 126.0}, "to": {"lat": 37.4979, "lon": 127.0276}}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["status"], 404);
    assert_eq!(body["code"], "ROUTE_NOT_FOUND");
    assert_eq!(body["message"], "경로 없음");
    assert!(body.get("data").is_none());

    // 위도 범위 밖과 없는 프로파일은 400 BAD_REQUEST, 없는 주소는 404 NOT_FOUND
    let (status, body) = send(
        &app,
        "POST",
        "/route",
        json!({"from": {"lat": 91.0, "lon": 126.0}, "to": {"lat": 37.4979, "lon": 127.0276}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "BAD_REQUEST");
    let (status, body) = send(
        &app,
        "POST",
        "/route",
        json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 37.4979, "lon": 127.0276}, "profile": "fast"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "BAD_REQUEST");
    assert!(
        body["message"]
            .as_str()
            .expect("message")
            .starts_with("본문이 잘못됨")
    );
    let (status, body) = send(&app, "GET", "/nothing", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
    assert_eq!(body["message"], "없는 주소: /nothing");
}

#[tokio::test]
async fn weight_updates_change_route() {
    let (app, db) = app_with_db();
    let body = json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 37.4979, "lon": 127.0276}, "profile": "time"});

    let (_, before) = send(&app, "POST", "/route", body.clone()).await;
    let links: Vec<u64> = before["data"]["links"]
        .as_array()
        .expect("links")
        .iter()
        .map(|l| l.as_u64().expect("LINK_ID"))
        .collect();
    let duration = before["data"]["duration_s"].as_f64().expect("duration_s");

    // 경로 가운데 링크를 전면 통제하면 그 링크를 피해 간다
    let blocked = links[links.len() / 2];
    let report = db.update_weights(&[WeightUpdate {
        link_id: blocked,
        speed_factor: None,
        incident_factor: Some(f32::INFINITY),
    }]);
    assert_eq!((report.applied, report.unknown, report.blocked), (1, 0, 1));
    eprintln!("update_weights 전면 통제 1건 {:.0}ms", report.elapsed_ms);
    let (status, after) = send(&app, "POST", "/route", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let after_links: Vec<u64> = after["data"]["links"]
        .as_array()
        .expect("links")
        .iter()
        .map(|l| l.as_u64().expect("LINK_ID"))
        .collect();
    assert!(!after_links.contains(&blocked));
    assert_ne!(after_links, links);

    // 통제를 풀고 모든 경로 링크의 속도 배율을 2로 하면 시간이 늘어난다. 거리 프로파일은 그대로이다
    db.reset_weights();
    let updates: Vec<WeightUpdate> = links
        .iter()
        .map(|&link_id| WeightUpdate {
            link_id,
            speed_factor: Some(2.0),
            incident_factor: None,
        })
        .collect();
    let report = db.update_weights(&updates);
    assert_eq!((report.applied, report.blocked), (links.len(), 0));
    eprintln!(
        "update_weights 속도 배율 {}건 {:.0}ms",
        links.len(),
        report.elapsed_ms
    );
    let (_, slow) = send(&app, "POST", "/route", body.clone()).await;
    assert!(slow["data"]["duration_s"].as_f64().expect("duration_s") > duration);
    let (_, distance) = send(
        &app,
        "POST",
        "/route",
        json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 37.4979, "lon": 127.0276}}),
    )
    .await;
    assert_eq!(
        distance["data"]["links"].as_array().map(Vec::len),
        Some(104)
    );

    // 되돌리면 처음 경로와 같다
    db.reset_weights();
    let (_, restored) = send(&app, "POST", "/route", body).await;
    assert_eq!(restored["data"]["links"], before["data"]["links"]);
    assert_eq!(restored["data"]["duration_s"].as_f64(), Some(duration));
}

#[tokio::test]
async fn update_channel_applies_and_broadcasts() {
    let (app, _db, state) = app_with_state();
    let body = json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 37.4979, "lon": 127.0276}, "profile": "time"});
    let (_, before) = send(&app, "POST", "/route", body.clone()).await;
    let links: Vec<u64> = before["data"]["links"]
        .as_array()
        .expect("links")
        .iter()
        .map(|l| l.as_u64().expect("LINK_ID"))
        .collect();

    // 폴링 태스크처럼 채널로 배율 묶음을 보내면 적용 태스크가 반영하고 이벤트를 보낸다
    let mut events = state.events.subscribe();
    let blocked = links[links.len() / 2];
    state
        .updates
        .send(Batch::Updates(vec![WeightUpdate {
            link_id: blocked,
            speed_factor: None,
            incident_factor: Some(f32::INFINITY),
        }]))
        .await
        .expect("채널 전송 실패");
    let event = tokio::time::timeout(std::time::Duration::from_secs(10), events.recv())
        .await
        .expect("이벤트 대기 시간 초과")
        .expect("이벤트 수신 실패");
    assert_eq!(
        (event.applied, event.blocked, event.sequence, event.reset),
        (1, 1, 1, false)
    );
    let (_, after) = send(&app, "POST", "/route", body).await;
    let after_links: Vec<u64> = after["data"]["links"]
        .as_array()
        .expect("links")
        .iter()
        .map(|l| l.as_u64().expect("LINK_ID"))
        .collect();
    assert!(!after_links.contains(&blocked));

    // 돌발 배율만 되돌리면 통제가 풀려 처음 경로로 돌아온다
    state
        .updates
        .send(Batch::Reset(routedb_core::route::FactorKind::Incident))
        .await
        .expect("채널 전송 실패");
    let event = tokio::time::timeout(std::time::Duration::from_secs(10), events.recv())
        .await
        .expect("이벤트 대기 시간 초과")
        .expect("이벤트 수신 실패");
    assert_eq!((event.blocked, event.sequence, event.reset), (0, 2, true));
    let (_, restored) = send(&app, "POST", "/route", json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 37.4979, "lon": 127.0276}, "profile": "time"})).await;
    assert_eq!(restored["data"]["links"], before["data"]["links"]);
}
