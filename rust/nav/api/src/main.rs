//! 내비 API 서버. `RouteDb`를 `Arc`로 공유하며 `POST /route`로 경로를 반환한다.
//!
//! cargo run --release -p nav-api -- [그래프 폴더] [주소]
//! 그래프 폴더는 moct.rdb, moct.cch-struct, moct.cch-metric, moct.cch-metric-time, moct.cch-metric-free가 있는 폴더이다.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use routedb_core::route::{Profile, Route, RouteDb};
use serde::{Deserialize, Serialize};

const DEFAULT_GRAPH_DIR: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb";
const DEFAULT_ADDR: &str = "127.0.0.1:3000";

/// 위경도
#[derive(Clone, Copy, Debug, Deserialize)]
struct LatLon {
    lat: f64,
    lon: f64,
}

/// `POST /route` 요청. 프로파일은 distance, time, free이며 없으면 distance이다
#[derive(Debug, Deserialize)]
struct RouteRequest {
    from: LatLon,
    to: LatLon,
    #[serde(default = "default_profile")]
    profile: Profile,
}

fn default_profile() -> Profile {
    Profile::Distance
}

/// 경로가 없을 때의 응답
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// 경로 응답. `Route`에 탐색 시간을 더한다
#[derive(Serialize)]
struct RouteResponse {
    #[serde(flatten)]
    route: Route,
    elapsed_ms: f64,
}

type Shared = Arc<RouteDb>;

async fn route_handler(
    State(db): State<Shared>,
    Json(req): Json<RouteRequest>,
) -> Result<Json<RouteResponse>, (StatusCode, Json<ErrorResponse>)> {
    // 탐색은 CPU 작업이므로 블로킹 스레드에서 실행한다
    let result = tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let cch_view = db.cch_view();
        let mut router = db.router(&cch_view);
        let route = router.route(
            (req.from.lat, req.from.lon),
            (req.to.lat, req.to.lon),
            req.profile,
        );
        (route, started.elapsed().as_secs_f64() * 1000.0)
    })
    .await
    .expect("탐색 작업이 중단됨");
    match result {
        (Some(route), elapsed_ms) => Ok(Json(RouteResponse { route, elapsed_ms })),
        (None, _) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "경로 없음".to_string(),
            }),
        )),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = Path::new(args.first().map_or(DEFAULT_GRAPH_DIR, String::as_str));
    let addr = args.get(1).map_or(DEFAULT_ADDR, String::as_str);

    let started = Instant::now();
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
    println!(
        "그래프와 CCH 번들 열기, R-tree 생성 {:.1}초",
        started.elapsed().as_secs_f64()
    );

    let app = Router::new()
        .route("/route", post(route_handler))
        .with_state(Arc::new(db));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("주소에 바인드하지 못함");
    println!("nav-api {addr}");
    axum::serve(listener, app).await.expect("서버 실행 실패");
}
