//! service: 검색어 검증, 브이월드 검색 API 호출, 응답 변환. HTTP 서버 쪽(axum)은 모른다

use serde_json::Value;

use super::model::{GeocodeItem, GeocodeQuery, GeocodeResponse, SearchType};
use crate::error::AppError;

const VWORLD_SEARCH_URL: &str = "https://api.vworld.kr/req/search";
const MAX_SIZE: u32 = 100;

/// 검색어를 검증하고 브이월드 검색 API를 호출하여 결과 목록을 만든다
pub async fn search(
    http: &reqwest::Client,
    key: &str,
    query: GeocodeQuery,
) -> Result<GeocodeResponse, AppError> {
    let q = query.q.trim();
    if q.is_empty() {
        return Err(AppError::BadRequest("q가 비어 있음".to_string()));
    }
    let size = query.size.clamp(1, MAX_SIZE).to_string();
    let mut params = vec![
        ("service", "search"),
        ("request", "search"),
        ("version", "2.0"),
        ("format", "json"),
        ("crs", "EPSG:4326"),
        ("key", key),
        ("query", q),
        ("size", &size),
    ];
    match query.search_type {
        SearchType::Place => params.push(("type", "PLACE")),
        SearchType::Address => {
            params.push(("type", "ADDRESS"));
            params.push(("category", "ROAD"));
        }
    }
    let body: Value = http
        .get(VWORLD_SEARCH_URL)
        .query(&params)
        .send()
        .await
        .map_err(|e| AppError::Upstream(format!("브이월드 호출 실패: {e}")))?
        .json()
        .await
        .map_err(|e| AppError::Upstream(format!("브이월드 응답이 JSON이 아님: {e}")))?;
    parse_response(&body)
}

/// 브이월드 검색 응답을 결과 목록으로 바꾼다. NOT_FOUND는 빈 목록이고 ERROR는 Upstream 에러이다
pub fn parse_response(body: &Value) -> Result<GeocodeResponse, AppError> {
    let response = &body["response"];
    match response["status"].as_str() {
        Some("OK") => {}
        Some("NOT_FOUND") => {
            return Ok(GeocodeResponse {
                total: 0,
                items: Vec::new(),
            });
        }
        _ => {
            let text = response["error"]["text"]
                .as_str()
                .unwrap_or("알 수 없는 응답");
            return Err(AppError::Upstream(format!("브이월드 오류: {text}")));
        }
    }
    let total = response["record"]["total"]
        .as_str()
        .and_then(|t| t.parse().ok())
        .or_else(|| response["record"]["total"].as_u64().map(|t| t as u32))
        .unwrap_or(0);
    let mut items = Vec::new();
    if let Some(list) = response["result"]["items"].as_array() {
        for item in list {
            let road = item["address"]["road"].as_str().unwrap_or("");
            let parcel = item["address"]["parcel"].as_str().unwrap_or("");
            let address = if road.is_empty() { parcel } else { road };
            let title = item["title"].as_str().unwrap_or(address);
            let (Some(lon), Some(lat)) = (number(&item["point"]["x"]), number(&item["point"]["y"]))
            else {
                continue;
            };
            items.push(GeocodeItem {
                title: title.to_string(),
                address: address.to_string(),
                lat,
                lon,
            });
        }
    }
    Ok(GeocodeResponse { total, items })
}

/// 브이월드는 좌표를 문자열로 주므로 문자열과 숫자 둘 다 받는다
fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_place_and_not_found() {
        let body = json!({"response": {"status": "OK", "record": {"total": "2", "current": "2"},
        "result": {"crs": "EPSG:4326", "type": "PLACE", "items": [
            {"id": "1", "title": "서울역", "category": "교통", "address": {"road": "서울특별시 중구 한강대로 405", "parcel": "서울특별시 중구 봉래동2가 122-11"}, "point": {"x": "126.9707", "y": "37.5547"}},
            {"id": "2", "title": "좌표 없음", "address": {"road": "", "parcel": "지번만"}, "point": {}}
        ]}}});
        let parsed = parse_response(&body).expect("OK 응답");
        assert_eq!(parsed.total, 2);
        assert_eq!(
            parsed.items,
            [GeocodeItem {
                title: "서울역".to_string(),
                address: "서울특별시 중구 한강대로 405".to_string(),
                lat: 37.5547,
                lon: 126.9707,
            }]
        );
        let none = json!({"response": {"status": "NOT_FOUND"}});
        assert_eq!(parse_response(&none).expect("빈 목록").items.len(), 0);
        let error = json!({"response": {"status": "ERROR", "error": {"text": "인증키가 틀림"}}});
        assert_eq!(
            parse_response(&error),
            Err(AppError::Upstream(
                "브이월드 오류: 인증키가 틀림".to_string()
            ))
        );
    }
}
