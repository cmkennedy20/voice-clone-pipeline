//! Top-level `Axum` route assembly mounting `/health`, `/api/v1`, `/v1`, and embedded Studio `SPA`.

use axum::Router;
use framework::context::AppContext;

use crate::controllers::{health_controller, spa_controller, v1};

/// Mounts health probes at `/health`, `v1` API routes at `/api/v1` and `/v1`, and `SPA` fallback.
pub fn build_routes() -> Router<AppContext> {
    Router::new()
        .nest("/health", health_controller::routes())
        .nest("/api/v1", v1::routes())
        .nest("/v1", v1::routes())
        .fallback(spa_controller::serve_spa_or_not_found)
}
