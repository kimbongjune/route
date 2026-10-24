//! 내비 API 서버. 그래프 파일을 열어 `AppState`를 만들고 lib.rs의 `app`으로 라우터를 조립한다.
//!
//! cargo run --release -p nav-api -- [그래프 폴더] [주소] [사고다발 폴리곤 CSV] [유도선 CSV]
//! 그래프 폴더는 moct.rdb, moct.cch-struct, moct.cch-metric, moct.cch-metric-time, moct.cch-metric-free와 지오코더 색인 geocode.idx가 있는 폴더이다.
//! 인자가 없으면 데이터 폴더(환경 변수 ROUTE_MAPS_DIR, 없으면 저장소의 maps) 아래의 기본 위치이고 주소는 NAV_ADDR, 없으면 127.0.0.1:3000이다.
//! 정적 파일 폴더는 NAV_WEB_DIR, 없으면 저장소의 nav/web이다.
//! 배경지도 타일용 브이월드 인증키는 환경 변수 VWORLD_API_KEY 또는 저장소 루트 .env의 VWORLD_API_KEY에서 읽는다.
//! 자체 벡터 타일은 GEOSERVER_URL(없으면 http://127.0.0.1:8080/geoserver)의 GeoServer에서 받아 전달한다.
//! 국가교통정보센터 인증키 ITS_API_KEY와 폴링 간격 TRAFFIC_POLL_SECS가 모두 있을 때만 소통정보 폴링 태스크를 실행하고,
//! INCIDENT_POLL_SECS가 있을 때만 돌발정보 폴링 태스크를 실행한다.
//! 인증키의 월 호출 한도가 1,000건이라 폴링 한 번(타일 16번)마다 한도를 쓰므로 기본으로는 폴링하지 않는다.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nav_api::accident::index::AccidentIndex;
use nav_api::collector;
use nav_api::guideline::GuideLineTable;
use nav_api::route::toll::TollTable;
use nav_api::state::AppState;
use routedb_core::route::RouteDb;
use routedb_geocode::GeocodeIndex;
use tokio::sync::broadcast;

/// `ROUTE_MAPS_DIR`이 없을 때의 데이터 폴더
const DEFAULT_MAPS_DIR: &str = r"C:\Users\nocdu\Desktop\dev\route\maps";
const DEFAULT_ADDR: &str = "127.0.0.1:3000";
const DEFAULT_GEOSERVER_URL: &str = "http://127.0.0.1:8080/geoserver";
const ENV_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../.env");

/// 데이터 폴더. 환경 변수 `ROUTE_MAPS_DIR`이 있으면 그 폴더이다
fn maps_dir() -> PathBuf {
    std::env::var_os("ROUTE_MAPS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MAPS_DIR))
}

/// 그래프 폴더의 파일 다섯 개로 `RouteDb`를 연다
pub fn open_route_db(dir: &Path) -> std::io::Result<RouteDb> {
    RouteDb::open(
        &dir.join("moct.rdb"),
        &dir.join("moct.cch-struct"),
        [
            &dir.join("moct.cch-metric"),
            &dir.join("moct.cch-metric-time"),
            &dir.join("moct.cch-metric-free"),
        ],
    )
}

/// 환경 변수에 없으면 .env 파일의 `이름=값` 줄에서 읽는다
pub fn env_value(name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(name) {
        return Some(value);
    }
    let text = std::fs::read_to_string(ENV_FILE).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix(&format!("{name}=")))
        .map(|value| {
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
        .next()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let maps = maps_dir();
    let dir = args
        .first()
        .map_or_else(|| maps.join("routedb"), PathBuf::from);
    let default_addr = std::env::var("NAV_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let addr = args.get(1).map_or(default_addr.as_str(), String::as_str);
    let accident_file = args.get(2).map_or_else(
        || {
            maps.join("전국교통사고다발지역표준데이터")
                .join("사고다발_폴리곤.csv")
        },
        PathBuf::from,
    );
    let guide_line_file = args.get(3).map_or_else(
        || maps.join("유도선").join("유도선_노드.csv"),
        PathBuf::from,
    );
    let vworld_key = env_value("VWORLD_API_KEY").expect("VWORLD_API_KEY가 없음");
    let geoserver_url = std::env::var("GEOSERVER_URL")
        .unwrap_or_else(|_| DEFAULT_GEOSERVER_URL.to_string())
        .trim_end_matches('/')
        .to_string();

    let started = Instant::now();
    let db = open_route_db(&dir).expect("그래프 파일을 열지 못함");
    println!(
        "그래프와 CCH 번들 열기, R-tree 생성 {:.1}초",
        started.elapsed().as_secs_f64()
    );
    let started = Instant::now();
    let accidents = AccidentIndex::load(&accident_file).expect("사고다발 폴리곤 파일을 읽지 못함");
    println!(
        "사고다발 폴리곤 {}개 R-tree 생성 {:.2}초",
        accidents.len(),
        started.elapsed().as_secs_f64()
    );

    let guide_lines = GuideLineTable::load(&guide_line_file).expect("유도선 파일을 읽지 못함");
    println!("노면색깔유도선 {}행", guide_lines.len());

    let toll_dir = maps.join("유료도로");
    let tolls = TollTable::load(&toll_dir.join("영업소.csv"), &toll_dir.join("요금표.csv"))
        .expect("통행료 표를 읽지 못함");
    println!(
        "통행료 영업소 {} 쌍 {}",
        tolls.unit_count(),
        tolls.pair_count()
    );

    let started = Instant::now();
    let geocoder = GeocodeIndex::open(&dir.join("geocode.idx")).expect("지오코더 색인을 열지 못함");
    println!(
        "지오코더 색인 열기 장소 {} 주소 {} {:.2}초",
        geocoder.poi_count(),
        geocoder.address_count(),
        started.elapsed().as_secs_f64()
    );

    let db = Arc::new(db);
    let http = reqwest::Client::new();
    let (events, _) = broadcast::channel(64);
    let updates = collector::start(Arc::clone(&db), events.clone());
    let poll_secs: Option<u64> = env_value("TRAFFIC_POLL_SECS").and_then(|v| v.parse().ok());
    match (env_value("ITS_API_KEY"), poll_secs) {
        (Some(its_key), Some(secs)) => {
            println!("소통정보 폴링 {secs}초 간격");
            tokio::spawn(collector::traffic::run(
                http.clone(),
                its_key,
                Arc::clone(&db),
                updates.clone(),
                Duration::from_secs(secs),
            ));
        }
        _ => println!("ITS_API_KEY 또는 TRAFFIC_POLL_SECS가 없어 소통정보 폴링을 하지 않음"),
    }
    let incident_secs: Option<u64> = env_value("INCIDENT_POLL_SECS").and_then(|v| v.parse().ok());
    match (env_value("ITS_API_KEY"), incident_secs) {
        (Some(its_key), Some(secs)) => {
            println!("돌발정보 폴링 {secs}초 간격");
            tokio::spawn(collector::incident::run(
                http.clone(),
                its_key,
                Arc::clone(&db),
                updates.clone(),
                Duration::from_secs(secs),
            ));
        }
        _ => println!("ITS_API_KEY 또는 INCIDENT_POLL_SECS가 없어 돌발정보 폴링을 하지 않음"),
    }
    let app = nav_api::app(AppState {
        db,
        accidents: Arc::new(accidents),
        guide_lines: Arc::new(guide_lines),
        geocoder: Arc::new(geocoder),
        tolls: Arc::new(tolls),
        http,
        vworld_key,
        geoserver_url,
        updates,
        events,
    });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("주소에 바인드하지 못함");
    println!("nav-api {addr}");
    axum::serve(listener, app).await.expect("서버 실행 실패");
}
