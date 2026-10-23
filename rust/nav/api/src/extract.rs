//! 요청 본문과 쿼리 추출기. axum의 `Json`, `Query`가 해석에 실패하면 본문 없는 400이나 422를 그대로 보내므로
//! 실패를 `AppError::BadRequest`로 바꾸어 공통 응답 구조로 나가게 한다

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Json, Query, Request};
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use crate::error::AppError;

/// JSON 본문. 해석 실패는 400 BAD_REQUEST
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => Err(json_error(rejection)),
        }
    }
}

/// 쿼리 문자열. 해석 실패는 400 BAD_REQUEST
pub struct ApiQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(ApiQuery(value)),
            Err(rejection) => Err(query_error(rejection)),
        }
    }
}

fn json_error(rejection: JsonRejection) -> AppError {
    AppError::BadRequest(format!("본문이 잘못됨: {}", rejection.body_text()))
}

fn query_error(rejection: QueryRejection) -> AppError {
    AppError::BadRequest(format!("쿼리가 잘못됨: {}", rejection.body_text()))
}
