use crate::error::Error;
use axum::{
    Json,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

pub type ApiResult<T> = Result<T, ApiError>;

pub const PROBLEM_JSON: &str = "application/problem+json";

#[derive(Serialize, ToSchema)]
pub struct Problem {
    status: u16,
    title: Option<&'static str>,
    detail: Option<String>,
}

pub struct ApiError(Error);

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Error::Forbidden(_) => StatusCode::FORBIDDEN,
            Error::Conflict(_) => StatusCode::CONFLICT,
            Error::Unexpected(cause) => {
                tracing::error!(cause);
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        let detail = (status != StatusCode::INTERNAL_SERVER_ERROR).then(|| self.0.to_string());
        let problem = Problem { status: status.as_u16(), title: status.canonical_reason(), detail };
        (status, [(CONTENT_TYPE, PROBLEM_JSON)], Json(problem)).into_response()
    }
}
