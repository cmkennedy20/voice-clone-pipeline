//! Thin `Axum` fallback controller serving embedded Studio `SPA` assets (`WEB_DIST_DIR`).

use axum::{
    body::Body,
    extract::State,
    http::{Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use framework::{
    context::AppContext,
    desktop::spa::{SpaAssetError, load_spa_asset},
};

use crate::error::ApiError;

/// Fallback handler delegating static `SPA` asset lookup to `framework::desktop::spa`.
pub async fn serve_spa_or_not_found(
    State(ctx): State<AppContext>,
    method: Method,
    uri: Uri,
) -> Result<Response, ApiError> {
    let req_path = uri.path();
    if method != Method::GET && method != Method::HEAD {
        return Err(ApiError::NotFound(format!("route `{req_path}` not found")));
    }
    let asset = load_spa_asset(ctx.config.desktop.web_dist_dir.as_deref(), req_path)
        .await
        .map_err(|err| match err {
            SpaAssetError::PathTraversal => ApiError::BadRequest(
                "invalid path traversal segment in static asset URI".to_owned(),
            ),
            SpaAssetError::NotFound => ApiError::NotFound(format!("route `{req_path}` not found")),
        })?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, asset.content_type),
            (header::CACHE_CONTROL, asset.cache_control),
        ],
        Body::from(asset.bytes),
    )
        .into_response())
}
