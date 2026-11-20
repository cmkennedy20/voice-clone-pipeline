//! Preserves incoming `x-request-id` headers or assigns a fresh `UUIDv7` correlation ID.

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

/// Canonical correlation `HTTP` header name (`x-request-id`).
pub const REQUEST_ID_HEADER: &str = "x-request-id";

fn extract_or_generate_request_id(request: &Request) -> HeaderValue {
    if let Some(existing) = request.headers().get(REQUEST_ID_HEADER)
        && let Ok(raw_str) = existing.to_str()
    {
        let trimmed = raw_str.trim();
        if !trimmed.is_empty()
            && let Ok(valid_val) = HeaderValue::from_str(trimmed)
        {
            return valid_val;
        }
    }

    let generated = Uuid::now_v7().to_string();
    HeaderValue::from_str(&generated).unwrap_or_else(|_| HeaderValue::from_static("req-fallback"))
}

/// `Axum` middleware preserving an incoming `x-request-id` or generating a `UUIDv7` on both
/// request and response headers.
pub async fn propagate_or_assign_request_id(mut request: Request, next: Next) -> Response {
    let header_name = HeaderName::from_static(REQUEST_ID_HEADER);
    let request_id = extract_or_generate_request_id(&request);

    request
        .headers_mut()
        .insert(header_name.clone(), request_id.clone());

    let mut response = next.run(request).await;
    response.headers_mut().insert(header_name, request_id);
    response
}

#[cfg(test)]
mod tests {
    use std::future::ready;

    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
    };
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::{REQUEST_ID_HEADER, propagate_or_assign_request_id};

    fn test_router() -> Router {
        Router::new()
            .route("/ping", get(|| ready(StatusCode::OK)))
            .layer(middleware::from_fn(propagate_or_assign_request_id))
    }

    #[tokio::test]
    async fn preserves_supplied_request_id_header() {
        let resp = test_router()
            .oneshot(
                Request::builder()
                    .uri("/ping")
                    .header(REQUEST_ID_HEADER, "client-trace-42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(REQUEST_ID_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some("client-trace-42")
        );
    }

    #[tokio::test]
    async fn generates_uuid_v7_when_request_id_header_is_missing() {
        let resp = test_router()
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let raw_id = resp
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap();
        let parsed = Uuid::parse_str(raw_id).unwrap();
        assert_eq!(parsed.get_version_num(), 7);
    }
}
