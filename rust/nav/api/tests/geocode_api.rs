//! 지오코딩 통합 테스트. 저장소 루트 .env의 VWORLD_API_KEY로 브이월드를 실제로 호출하므로 키가 없으면 건너뛴다.
//! 그래프 파일이 필요 없도록 라우터 대신 service를 직접 호출한다.

use nav_api::error::AppError;
use nav_api::geocode::model::{GeocodeQuery, SearchType};
use nav_api::geocode::service;

const ENV_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../.env");

fn vworld_key() -> Option<String> {
    let text = std::fs::read_to_string(ENV_FILE).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix("VWORLD_API_KEY="))
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn query(q: &str, search_type: SearchType) -> GeocodeQuery {
    GeocodeQuery {
        q: q.to_string(),
        search_type,
        size: 5,
    }
}

#[tokio::test]
async fn geocode_requests() {
    let Some(key) = vworld_key() else {
        eprintln!("VWORLD_API_KEY가 없어 건너뜀");
        return;
    };
    let http = reqwest::Client::new();

    // 장소명 검색. 서울역이 첫 결과이고 좌표가 서울 안이다
    let found = service::search(&http, &key, query("서울역", SearchType::Place))
        .await
        .expect("장소 검색");
    assert!(found.total >= 1);
    let first = &found.items[0];
    assert!(first.title.contains("서울역"), "{}", first.title);
    assert!((37.0..38.0).contains(&first.lat) && (126.0..128.0).contains(&first.lon));

    // 도로명주소 검색
    let found = service::search(
        &http,
        &key,
        query("서울특별시 중구 한강대로 405", SearchType::Address),
    )
    .await
    .expect("주소 검색");
    assert!(!found.items.is_empty());
    assert!(found.items[0].address.contains("한강대로"));

    // 없는 장소는 빈 목록, 빈 검색어는 400
    let none = service::search(
        &http,
        &key,
        query("ㅋㅋㅋㅋㅋ없는장소명", SearchType::Place),
    )
    .await
    .expect("없는 장소");
    assert_eq!(none.items.len(), 0);
    assert_eq!(
        service::search(&http, &key, query("  ", SearchType::Place)).await,
        Err(AppError::BadRequest("q가 비어 있음".to_string()))
    );
}
