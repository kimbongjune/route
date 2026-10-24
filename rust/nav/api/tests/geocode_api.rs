//! 지오코딩 통합 테스트. 작은 색인 파일을 만들어 service를 직접 호출한다

use nav_api::error::AppError;
use nav_api::geocode::model::GeocodeQuery;
use nav_api::geocode::service;
use routedb_geocode::index::NO_LI;
use routedb_geocode::{Address, Builder, GeocodeIndex, Poi, Region, Road};

/// 서울 중구 세종대로 110 시청과 장소 둘
fn demo_index(name: &str) -> (GeocodeIndex, std::path::PathBuf) {
    let mut b = Builder::default();
    b.sido.push(Region {
        code: 11,
        name: "서울특별시".into(),
    });
    b.sig.push(Region {
        code: 11140,
        name: "중구".into(),
    });
    b.emd.push(Region {
        code: 11140103,
        name: "태평로1가".into(),
    });
    b.roads.push(Road {
        sig: 11140,
        rn: 2001,
        name: "세종대로".into(),
    });
    b.addresses.push(Address {
        road: 0,
        under: 0,
        mnnm: 110,
        slno: 0,
        lat: 37.5666,
        lon: 126.9784,
        emd: 0,
        li: NO_LI,
        san: 0,
        lnbr_mnnm: 31,
        lnbr_slno: 0,
        name: "서울특별시청".into(),
    });
    b.pois.push(Poi {
        name: "서울역".into(),
        address: "서울특별시 용산구 한강대로 405".into(),
        lat: 37.5547,
        lon: 126.9707,
        class: 403003010900,
        aliases: vec!["서울역KTX".into()],
    });
    b.pois.push(Poi {
        name: "서울특별시청".into(),
        address: "서울특별시 중구 세종대로 110".into(),
        lat: 37.5666,
        lon: 126.9784,
        class: 103002000000,
        aliases: vec!["시청".into()],
    });
    let path = std::env::temp_dir().join(format!("nav_geocode_{name}_{}.idx", std::process::id()));
    b.write(&path).unwrap();
    (GeocodeIndex::open(&path).unwrap(), path)
}

fn query(q: &str) -> GeocodeQuery {
    GeocodeQuery {
        q: q.to_string(),
        size: 5,
        ..Default::default()
    }
}

#[test]
fn geocode_requests() {
    let (index, path) = demo_index("search");

    // 장소명 검색. 서울역이 첫 결과이고 종류가 place이다
    let found = service::search(&index, query("서울역")).unwrap();
    assert_eq!(found.total, 1);
    assert_eq!(
        (found.items[0].kind, found.items[0].title.as_str()),
        ("place", "서울역")
    );
    assert_eq!(found.items[0].address, "서울특별시 용산구 한강대로 405");

    // 도로명주소 검색은 address이고 장소 시청은 뒤에 온다
    let found = service::search(&index, query("서울 중구 세종대로 110")).unwrap();
    assert_eq!(found.items[0].kind, "address");
    assert_eq!(found.items[0].title, "서울특별시 중구 세종대로 110");
    assert_eq!(
        found.items[0].address,
        "서울특별시 중구 태평로1가 31 (서울특별시청)"
    );

    // 지도 중심이 시청이면 "서울"은 시청이 먼저다
    let found = service::search(
        &index,
        GeocodeQuery {
            lat: Some(37.5666),
            lon: Some(126.9784),
            ..query("서울")
        },
    )
    .unwrap();
    assert_eq!(found.items[0].title, "서울특별시청");

    // 없는 장소는 빈 목록, 빈 검색어와 좌표 하나만 있는 요청은 400
    assert_eq!(
        service::search(&index, query("없는장소명"))
            .unwrap()
            .items
            .len(),
        0
    );
    assert_eq!(
        service::search(&index, query("  ")),
        Err(AppError::BadRequest("q가 비어 있음".to_string()))
    );
    assert_eq!(
        service::search(
            &index,
            GeocodeQuery {
                lat: Some(37.5),
                ..query("서울")
            }
        ),
        Err(AppError::BadRequest(
            "lat과 lon은 같이 있어야 함".to_string()
        ))
    );

    // 역지오코딩. 시청 옆은 세종대로 110이고 바다는 404
    let item = service::reverse(&index, 37.5665, 126.9785).unwrap();
    assert_eq!(item.title, "서울특별시 중구 세종대로 110");
    assert_eq!(
        service::reverse(&index, 35.0, 129.5),
        Err(AppError::AddressNotFound)
    );
    assert_eq!(
        service::reverse(&index, 91.0, 0.0),
        Err(AppError::BadRequest(
            "좌표 범위를 벗어남: 91, 0".to_string()
        ))
    );

    drop(index);
    std::fs::remove_file(path).unwrap();
}
