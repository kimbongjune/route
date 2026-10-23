//! API 통합 테스트. 서버를 띄우지 않고 라우터에 HTTP 요청을 직접 보낸다.
//! 전국 그래프 파일을 열므로 maps/routedb가 있어야 한다.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use nav_api::accident::index::AccidentIndex;
use nav_api::state::AppState;
use routedb_core::route::RouteDb;
use serde_json::{Value, json};
use tower::ServiceExt;

const GRAPH_DIR: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb";
const ACCIDENT_FILE: &str =
    r"C:\Users\nocdu\Desktop\dev\route\maps\전국교통사고다발지역표준데이터\사고다발_폴리곤.csv";

fn app() -> Router {
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
    nav_api::app(AppState {
        db: Arc::new(db),
        accidents: Arc::new(accidents),
        http: reqwest::Client::new(),
        vworld_key: String::new(),
    })
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

    // 서울역→부산역 최소시간 경로는 영업소 4곳을 지난다
    let (status, body) = send(
        &app,
        "POST",
        "/route",
        json!({"from": {"lat": 37.5547, "lon": 126.9707}, "to": {"lat": 35.1151, "lon": 129.0422}, "profile": "time"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["tolls"].as_array().map(Vec::len), Some(4));

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
