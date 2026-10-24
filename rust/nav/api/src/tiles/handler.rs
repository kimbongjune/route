//! `GET /tiles/{layer}/{z}/{x}/{y}`: 브이월드 WMTS 타일을, `GET /vtiles/{layer}/{z}/{x}/{y}`: GeoServer 벡터 타일을 그대로 전달한다

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

/// 자체 벡터 타일 레이어. GeoServer의 emap 워크스페이스 뷰 레이어 이름
fn vector_layer_of(name: &str) -> Option<&'static str> {
    match name {
        "buld" => Some("v_buld"),
        "road" => Some("v_road"),
        "road_major" => Some("v_road_major"),
        "rw" => Some("v_rw"),
        "sig" => Some("v_sig"),
        "emd" => Some("v_emd"),
        _ => None,
    }
}

/// GeoServer GeoWebCache의 WMTS에서 벡터 타일을 받아 전달한다. 범위 밖이나 빈 타일은 GeoServer가 주는 상태 코드를 그대로 돌려준다
pub async fn vector_tile(
    State(state): State<AppState>,
    Path((layer, z, x, y)): Path<(String, u8, u32, u32)>,
) -> Result<Response, AppError> {
    let Some(gs_layer) = vector_layer_of(&layer) else {
        return Err(AppError::BadRequest(format!(
            "벡터 타일 레이어 {layer}는 없음. buld, road, road_major, rw, sig, emd 중 하나"
        )));
    };
    let url = format!(
        "{}/gwc/service/wmts?REQUEST=GetTile&SERVICE=WMTS&VERSION=1.0.0&LAYER=emap:{gs_layer}&STYLE=\
         &TILEMATRIXSET=EPSG:900913&TILEMATRIX=EPSG:900913:{z}&TILECOL={x}&TILEROW={y}&FORMAT={MVT_CONTENT_TYPE}",
        state.geoserver_url
    );
    let upstream = state
        .http
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Upstream(format!("GeoServer 타일 호출 실패: {e}")))?;
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if status == StatusCode::OK {
        let bytes = upstream
            .bytes()
            .await
            .map_err(|e| AppError::Upstream(format!("GeoServer 타일 본문 읽기 실패: {e}")))?;
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
    // 격자 범위 밖(400)이나 빈 타일(204)은 본문 없이 상태 코드만 전달한다
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
