//! handler: HTTP 요청을 받아 service를 호출하고 응답을 만든다. 탐색 규칙은 넣지 않는다

use axum::extract::State;

use super::model::{RouteRequest, RouteResponse, RoutesResponse};
use super::service;
use crate::error::AppError;
use crate::extract::ApiJson;
use crate::response::ApiResponse;
use crate::state::AppState;

/// `POST /route`
pub async fn create(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<RouteRequest>,
) -> Result<ApiResponse<RouteResponse>, AppError> {
    let response = service::find_route(
        state.db,
        state.accidents,
        state.guide_lines,
        state.tolls,
        input,
    )
    .await?;
    Ok(ApiResponse::ok(response))
}

/// `POST /routes`: 세 프로파일의 후보를 통행료와 함께 비교한다
pub async fn compare(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<RouteRequest>,
) -> Result<ApiResponse<RoutesResponse>, AppError> {
    let response = service::find_routes(
        state.db,
        state.accidents,
        state.guide_lines,
        state.tolls,
        input,
    )
    .await?;
    Ok(ApiResponse::ok(response))
}
