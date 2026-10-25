//! 데이터 파일 위치. 환경 변수 `ROUTE_MAPS_DIR`이 있으면 그 폴더, 없으면 저장소의 maps 폴더 아래이다

use std::path::PathBuf;

/// `ROUTE_MAPS_DIR`이 없을 때의 데이터 폴더
pub const DEFAULT_MAPS_DIR: &str = r"C:\Users\nocdu\Desktop\dev\route\maps";

/// 데이터 폴더
pub fn maps_dir() -> PathBuf {
    std::env::var_os("ROUTE_MAPS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MAPS_DIR))
}

/// 표준노드링크 shp 폴더
pub fn moct_folder() -> PathBuf {
    maps_dir().join("[2026-10-14]NODELINKDATA")
}

/// 요금지점_링크.csv
pub fn toll_links() -> PathBuf {
    maps_dir().join("유료도로").join("요금지점_링크.csv")
}

/// 카메라_링크.csv
pub fn camera_links() -> PathBuf {
    maps_dir()
        .join("전국무인교통단속카메라표준데이터")
        .join("카메라_링크.csv")
}

/// 과속방지턱_링크.csv
pub fn bump_links() -> PathBuf {
    maps_dir()
        .join("과속방지턱정보")
        .join("행안부")
        .join("과속방지턱_링크.csv")
}

/// 골목 입력 CSV 폴더. 시도별 하위 폴더에 노드·링크 CSV가 있다
#[allow(dead_code)]
pub fn alley_dir() -> PathBuf {
    maps_dir().join("골목")
}

/// 골목 파일 alley.rdb
#[allow(dead_code)]
pub fn alley_file() -> PathBuf {
    routedb_dir().join("alley.rdb")
}

/// 그래프 파일과 CCH 파일을 저장하는 폴더
pub fn routedb_dir() -> PathBuf {
    maps_dir().join("routedb")
}

/// 그래프 파일 moct.rdb
pub fn graph_file() -> PathBuf {
    routedb_dir().join("moct.rdb")
}

/// CCH 구조 파일
pub fn cch_struct() -> PathBuf {
    routedb_dir().join("moct.cch-struct")
}

/// 거리 metric 파일
pub fn cch_metric() -> PathBuf {
    routedb_dir().join("moct.cch-metric")
}

/// 최소시간 metric 파일
pub fn cch_metric_time() -> PathBuf {
    routedb_dir().join("moct.cch-metric-time")
}

/// 무료도로 metric 파일
pub fn cch_metric_free() -> PathBuf {
    routedb_dir().join("moct.cch-metric-free")
}
