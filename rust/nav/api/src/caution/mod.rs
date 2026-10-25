//! 주의운전 구간. 상습 결빙구간, 안개 잦은 구간처럼 시작점과 끝점이 있는 구간(section)과 중심점 반경의 보호구역(zone)을
//! 경로 좌표 열과 맞추어 경고 구간을 찾는다

pub mod construction;
pub mod enforce;
pub mod rest;
pub mod risk;
pub mod section;
pub mod zone;
