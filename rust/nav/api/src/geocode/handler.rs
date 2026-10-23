//! handler: 검색어를 받아 service를 호출하고 응답을 만든다

use axum::extract::State;

use super::model::{GeocodeQuery, GeocodeResponse};
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
    let response = service::search(&state.http, &state.vworld_key, query).await?;
    Ok(ApiResponse::ok(response))
}
