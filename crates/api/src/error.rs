//! `RFC 7807` `Problem Details` error transport for `HTTP` API responses.

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use features::error::AppError;
use serde::{Deserialize, Serialize};

/// Media type mandated by `RFC 7807` (`application/problem+json`).
pub const PROBLEM_JSON_CONTENT_TYPE: &str = "application/problem+json";

/// Wire representation of an `RFC 7807` `Problem Details` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
}

/// Transport-layer `HTTP` error mapped deterministically to `RFC 7807` responses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    /// Returns the `HTTP` [`StatusCode`] and `RFC 7807` [`ProblemDetails`] descriptor.
    pub fn into_problem_details(self) -> (StatusCode, ProblemDetails) {
        let (status, slug, title, detail) = match self {
            Self::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Unauthorized",
                msg,
            ),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", "Forbidden", msg),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad-request", "Bad Request", msg),
            Self::Validation(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation-failed",
                "Validation Failed",
                msg,
            ),
            Self::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                "not-found",
                "Resource Not Found",
                msg,
            ),
            Self::Conflict(msg) => (StatusCode::CONFLICT, "conflict", "Conflict", msg),
            Self::ServiceUnavailable(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service-unavailable",
                "Service Unavailable",
                msg,
            ),
            Self::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "Internal Server Error",
                msg,
            ),
        };

        let problem = ProblemDetails {
            problem_type: format!("urn:voice-clone-pipeline:problem:{slug}"),
            title: title.to_owned(),
            status: status.as_u16(),
            detail,
        };
        (status, problem)
    }
}

impl From<AppError> for ApiError {
    fn from(value: AppError) -> Self {
        match value {
            AppError::Validation(msg) => Self::Validation(msg),
            AppError::NotFound(msg) => Self::NotFound(msg),
            AppError::Conflict(msg) => Self::Conflict(msg),
            AppError::Forbidden(msg) => Self::Forbidden(msg),
            AppError::Internal(msg) => Self::Internal(msg),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, problem) = self.into_problem_details();
        let mut response = (status, Json(problem)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(PROBLEM_JSON_CONTENT_TYPE),
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use features::error::AppError;
    use http_body_util::BodyExt;

    use super::{ApiError, PROBLEM_JSON_CONTENT_TYPE, ProblemDetails};

    #[tokio::test]
    async fn maps_app_errors_to_rfc7807_problem_json_responses() {
        let cases = [
            (
                ApiError::from(AppError::Validation("clip silent".to_owned())),
                StatusCode::UNPROCESSABLE_ENTITY,
                422_u16,
                "urn:voice-clone-pipeline:problem:validation-failed",
                "Validation Failed",
                "clip silent",
            ),
            (
                ApiError::from(AppError::NotFound("profile missing".to_owned())),
                StatusCode::NOT_FOUND,
                404_u16,
                "urn:voice-clone-pipeline:problem:not-found",
                "Resource Not Found",
                "profile missing",
            ),
            (
                ApiError::from(AppError::Conflict("duplicate slug".to_owned())),
                StatusCode::CONFLICT,
                409_u16,
                "urn:voice-clone-pipeline:problem:conflict",
                "Conflict",
                "duplicate slug",
            ),
            (
                ApiError::from(AppError::Forbidden("not owner".to_owned())),
                StatusCode::FORBIDDEN,
                403_u16,
                "urn:voice-clone-pipeline:problem:forbidden",
                "Forbidden",
                "not owner",
            ),
            (
                ApiError::from(AppError::Internal("cer exhausted".to_owned())),
                StatusCode::INTERNAL_SERVER_ERROR,
                500_u16,
                "urn:voice-clone-pipeline:problem:internal-error",
                "Internal Server Error",
                "cer exhausted",
            ),
        ];

        for (err, expected_status, expected_code, expected_urn, expected_title, expected_detail) in
            cases
        {
            let response = err.into_response();
            assert_eq!(response.status(), expected_status);
            assert_eq!(
                response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok()),
                Some(PROBLEM_JSON_CONTENT_TYPE)
            );

            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let problem: ProblemDetails = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(problem.problem_type, expected_urn);
            assert_eq!(problem.title, expected_title);
            assert_eq!(problem.status, expected_code);
            assert_eq!(problem.detail, expected_detail);
        }
    }
}
