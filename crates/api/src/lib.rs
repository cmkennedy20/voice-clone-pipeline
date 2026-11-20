//! Tier 5 thin `Axum` `HTTP` transport, `RFC 7807` `Problem Details`, and `AuthenticatedOwner`
//! request guard.

pub use axum;

use axum::{Router, extract::DefaultBodyLimit, middleware as axum_middleware};
use framework::context::AppContext;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub mod controllers;
pub mod error;
pub mod extractors;
pub mod middleware;
pub mod render_io;
pub mod routes;

/// Maximum `HTTP` request body size (`32 MiB`) accommodating `24 kHz` / `48 kHz` `WAV` uploads.
pub const MAX_UPLOAD_BODY_BYTES: usize = 32 * 1024 * 1024;

/// Assembles the stateless `Axum` [`Router`] wired to [`AppContext`], `32 MiB` body limit,
/// permissive `CORS`, `x-request-id` correlation middleware, and structured `HTTP` tracing.
pub fn build_router(ctx: AppContext) -> Router {
    routes::build_routes()
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES))
        .layer(axum_middleware::from_fn(
            middleware::request_id::propagate_or_assign_request_id,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(ctx)
}
