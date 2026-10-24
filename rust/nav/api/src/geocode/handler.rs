//! handler: 검색어를 받아 service를 호출하고 응답을 만든다

use axum::extract::State;

use super::model::{GeocodeItem, GeocodeQuery, GeocodeResponse, ReverseQuery};
use super::service;
use crate::error::AppError;
use crate::extract::ApiQuery;
use crate::response::ApiResponse;
use crate::state::AppState;

/// `GET /geocode`
pub async fn search(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<GeocodeQuery>,
) -> Result<ApiResponse<GeocodeResponse>, AppError> {
    let response = service::search(&state.geocoder, query)?;
    Ok(ApiResponse::ok(response))
}

/// `GET /reverse`
pub async fn reverse(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ReverseQuery>,
) -> Result<ApiResponse<GeocodeItem>, AppError> {
    let item = service::reverse(&state.geocoder, query.lat, query.lon)?;
    Ok(ApiResponse::ok(item))
}
