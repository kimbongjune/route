// 출발 위경도와 도착 위경도를 받아 RouteDb로 경로를 구하고 거리, 시간, 링크 열, 영업소, 통과제한, 지점을 출력한다.
// 다섯 번째 인자는 프로파일이며 distance, time, free 중 하나이고 없으면 distance이다.
// cargo run --release -p importer-knl --example route_cli -- 37.5547 126.9707 37.4979 127.0276 time
use std::path::Path;
use std::time::Instant;

use routedb_core::route::{PoiKind, Profile, RouteDb, SignColor};

const GRAPH_FILE_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.rdb";
const CCH_STRUCT_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-struct";
const CCH_METRIC_LOCATION: &str = r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-metric";
const CCH_METRIC_TIME_LOCATION: &str =
    r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-metric-time";
const CCH_METRIC_FREE_LOCATION: &str =
    r"C:\Users\nocdu\Desktop\dev\route\maps\routedb\moct.cch-metric-free";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let coords: Vec<f64> = args
        .iter()
        .take(4)
        .map(|a| a.parse().expect("위경도는 숫자여야 함"))
        .collect();
    let [from_lat, from_lon, to_lat, to_lon] = coords[..] else {
        eprintln!("사용법: route_cli 출발위도 출발경도 도착위도 도착경도 [distance|time|free]");
        std::process::exit(1);
    };
    let profile = match args.get(4).map(String::as_str) {
        None | Some("distance") => Profile::Distance,
        Some("time") => Profile::Time,
        Some("free") => Profile::Free,
        Some(other) => {
            eprintln!("프로파일 {other}는 없음. distance, time, free 중 하나");
            std::process::exit(1);
        }
    };

    let started = Instant::now();
    let db = RouteDb::open(
        Path::new(GRAPH_FILE_LOCATION),
        Path::new(CCH_STRUCT_LOCATION),
        [
            Path::new(CCH_METRIC_LOCATION),
            Path::new(CCH_METRIC_TIME_LOCATION),
            Path::new(CCH_METRIC_FREE_LOCATION),
        ],
    )
    .unwrap();
    let cch_view = db.cch_view();
    let mut router = db.router(&cch_view);
    println!(
        "그래프와 CCH 번들 열기, R-tree 생성 {:.1}초",
        started.elapsed().as_secs_f64()
    );

    let started = Instant::now();
    let Some(route) = router.route((from_lat, from_lon), (to_lat, to_lon), profile) else {
        println!("경로 없음");
        return;
    };
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let link_id = db.file.link_id();
    println!(
        "출발 스냅 링크 {} 거리 {:.1}m | 도착 스냅 링크 {} 거리 {:.1}m",
        link_id[route.from.edge as usize],
        route.from.distance_m,
        link_id[route.to.edge as usize],
        route.to.distance_m
    );
    println!(
        "{}",
        route
            .links
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!(
        "거리 {:.0}m 시간 {:.0}분 링크 {}개 좌표 {}개 ({:.0}ms)",
        route.distance_m,
        route.duration_s / 60.0,
        route.links.len(),
        route.geometry.len(),
        elapsed
    );
    println!(
        "영업소 {}",
        route
            .tolls
            .iter()
            .map(|u| format!("{u:03}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!(
        "통과제한 높이 {} 하중 {} 통행제한차량 {:?}",
        if route.min_height_cm > 0 {
            format!("{}cm", route.min_height_cm)
        } else {
            "없음".to_string()
        },
        if route.min_weight_kg > 0 {
            format!("{}kg", route.min_weight_kg)
        } else {
            "없음".to_string()
        },
        route.rest_veh
    );
    let cameras: Vec<_> = route
        .pois
        .iter()
        .filter(|p| p.kind == PoiKind::Camera)
        .collect();
    let bumps = route
        .pois
        .iter()
        .filter(|p| p.kind == PoiKind::Bump)
        .count();
    println!("카메라 {} 방지턱 {}", cameras.len(), bumps);
    println!("안내 지점 {}", route.steps.len());
    let name_or = |name: &str| {
        if name.is_empty() {
            "(이름 없음)".to_string()
        } else {
            name.to_string()
        }
    };
    for step in route.steps.iter() {
        let clock = match step.clock {
            Some(h) => format!(" {}시 방향 분기 {}", h, step.branches),
            None => format!(" 분기 {}", step.branches),
        };
        let sign = match step.sign {
            SignColor::Green => "녹색",
            SignColor::Blue => "청색",
        };
        println!(
            "  {:.0}m {:?} {:.0}°{} {} → {} {} {} @{} ({:.6}, {:.6})",
            step.at_m,
            step.turn,
            step.angle,
            clock,
            name_or(&step.from_name),
            name_or(&step.to_name),
            step.to_rank,
            sign,
            name_or(&step.node_name),
            step.lat,
            step.lon
        );
    }
    println!("도로명 구간 {}", route.segments.len());
    for seg in route.segments.iter().take(12) {
        let name = if seg.name.is_empty() {
            "(이름 없음)"
        } else {
            &seg.name
        };
        println!(
            "  {:.0}m~{:.0}m {} 링크 {}",
            seg.from_m, seg.to_m, name, seg.links
        );
    }
    for p in cameras.iter().take(10) {
        println!(
            "  {:.0}m 카메라 {} 제한속도 {} 단속 {} ({:.6}, {:.6})",
            p.at_m, p.id, p.limit, p.enforce, p.lat, p.lon
        );
    }
}
