//! Unauthenticated container liveness (`GET /health/live`) and `PostgreSQL` readiness
//! (`GET /health/ready`) endpoints.

use std::future::{Ready, ready};

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use framework::{context::AppContext, db::pool::check_db_ready};
use serde::Serialize;

use crate::error::ApiError;

/// Minimal health probe response body.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HealthStatus {
    pub status: &'static str,
}

/// Mounts `/live` and `/ready` probe routes.
pub fn routes() -> Router<AppContext> {
    Router::new()
        .route("/live", get(get_live))
        .route("/ready", get(get_ready))
}

fn get_live() -> Ready<(StatusCode, Json<HealthStatus>)> {
    ready((StatusCode::OK, Json(HealthStatus { status: "ok" })))
}

async fn get_ready(
    State(ctx): State<AppContext>,
) -> Result<(StatusCode, Json<HealthStatus>), ApiError> {
    check_db_ready(&ctx.db)
        .await
        .map_err(|err| ApiError::ServiceUnavailable(format!("database not ready: {err}")))?;
    Ok((StatusCode::OK, Json(HealthStatus { status: "ready" })))
}
