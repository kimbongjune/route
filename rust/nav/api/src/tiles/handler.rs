//! `GET /tiles/{layer}/{z}/{x}/{y}`: 브이월드 WMTS 타일을, `GET /vtiles/{layer}/{z}/{x}/{y}`: MapCache(MapServer) 벡터 타일을 그대로 전달한다

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::state::AppState;

const VWORLD_WMTS_URL: &str = "https://api.vworld.kr/req/wmts/1.0.0";
/// 브라우저가 타일을 다시 받지 않도록 하루 동안 캐시한다
const CACHE_CONTROL: &str = "public, max-age=86400";
const MVT_CONTENT_TYPE: &str = "application/vnd.mapbox-vector-tile";

/// 자체 벡터 타일 레이어. docker/mapserver/emap.map의 레이어(PostGIS emap 뷰) 이름
fn vector_layer_of(name: &str) -> Option<&'static str> {
    match name {
        "buld" => Some("v_buld"),
        "road" => Some("v_road"),
        "road_major" => Some("v_road_major"),
        "rw" => Some("v_rw"),
        "sig" => Some("v_sig"),
        "emd" => Some("v_emd"),
        "water_low" => Some("v_water_low"),
        "water" => Some("v_water"),
        "rail" => Some("v_rail2"),
        "subway" => Some("v_subway"),
        "link_z7" => Some("v_link_z7"),
        "link_low" => Some("v_link_low"),
        "link" => Some("v_link"),
        "station" => Some("v_station"),
        "forest_low" => Some("v_forest_low"),
        "forest" => Some("v_forest"),
        "land_extra" => Some("v_land_extra"),
        _ => None,
    }
}

/// MapCache에서 벡터 타일을 받아 전달한다. 빈 타일도 MapCache가 200으로 주고, 그 밖의 상태 코드는 본문 없이 그대로 돌려준다
pub async fn vector_tile(
    State(state): State<AppState>,
    Path((layer, z, x, y)): Path<(String, u8, u32, u32)>,
) -> Result<Response, AppError> {
    let Some(ms_layer) = vector_layer_of(&layer) else {
        return Err(AppError::BadRequest(format!(
            "벡터 타일 레이어 {layer}는 없음. buld, road, road_major, rw, sig, emd, water_low, water, rail, station, forest_low, forest, land_extra, subway, link_z7, link_low, link 중 하나"
        )));
    };
    let url = format!(
        "{}/mapcache/gmaps/{ms_layer}@GoogleMapsCompatible/{z}/{x}/{y}.pbf",
        state.mapserver_url
    );
    let upstream = state
        .http
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Upstream(format!("MapCache 타일 호출 실패: {e}")))?;
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if status == StatusCode::OK {
        let bytes = upstream
            .bytes()
            .await
            .map_err(|e| AppError::Upstream(format!("MapCache 타일 본문 읽기 실패: {e}")))?;
        let mut response = (StatusCode::OK, Body::from(bytes)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(MVT_CONTENT_TYPE),
        );
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(CACHE_CONTROL),
        );
        return Ok(response);
    }
    Ok(status.into_response())
}

/// 브이월드 타일 레이어. Base·midnight·Hybrid는 png, Satellite는 jpeg이다. gray는 XML 오류를 반환하여 뺐다
fn layer_of(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match name {
        "base" => Some(("Base", "png", "image/png")),
        "midnight" => Some(("midnight", "png", "image/png")),
        "hybrid" => Some(("Hybrid", "png", "image/png")),
        "satellite" => Some(("Satellite", "jpeg", "image/jpeg")),
        _ => None,
    }
}

pub async fn tile(
    State(state): State<AppState>,
    Path((layer, z, x, y)): Path<(String, u8, u32, u32)>,
) -> Result<Response, AppError> {
    let Some((vworld_layer, ext, content_type)) = layer_of(&layer) else {
        return Err(AppError::BadRequest(format!(
            "타일 레이어 {layer}는 없음. base, midnight, hybrid, satellite 중 하나"
        )));
    };
    let url = format!(
        "{VWORLD_WMTS_URL}/{}/{vworld_layer}/{z}/{y}/{x}.{ext}",
        state.vworld_key
    );
    let upstream = state
        .http
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Upstream(format!("브이월드 타일 호출 실패: {e}")))?;
    if upstream.status() != reqwest::StatusCode::OK {
        return Err(AppError::Upstream(format!(
            "브이월드 타일 응답 {}",
            upstream.status()
        )));
    }
    // 브이월드는 범위 밖 타일에도 200으로 XML 오류를 주므로 content-type이 이미지가 아니면 오류로 본다
    let is_image = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("image/"));
    let bytes = upstream
        .bytes()
        .await
        .map_err(|e| AppError::Upstream(format!("브이월드 타일 본문 읽기 실패: {e}")))?;
    if !is_image {
        let text = String::from_utf8_lossy(&bytes);
        let message = text
            .split("<![CDATA[")
            .nth(1)
            .and_then(|t| t.split("]]>").next())
            .unwrap_or("이미지가 아닌 응답");
        return Err(AppError::Upstream(format!("브이월드 타일 오류: {message}")));
    }
    let mut response = (StatusCode::OK, Body::from(bytes)).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_CONTROL),
    );
    Ok(response)
}
