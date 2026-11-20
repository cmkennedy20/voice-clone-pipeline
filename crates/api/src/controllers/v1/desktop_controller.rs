//! Thin `v1` controllers for agent discovery (`GET /agent/manifest`) and desktop onboarding
//! (`GET /system/status`, `POST /system/discord-config`).

use std::future::{Ready, ready};

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, post},
};
use features::desktop::{
    AgentManifestResponse, DesktopSystemStatusResponse, SaveDiscordConfigRequest,
    SaveDiscordConfigResponse, get_agent_manifest, get_desktop_system_status, save_discord_config,
};
use framework::context::AppContext;

use crate::{error::ApiError, extractors::AuthenticatedOwner};

/// Mounts `/agent/manifest`, `/system/status`, and `/system/discord-config`.
pub fn routes() -> Router<AppContext> {
    Router::new()
        .route("/agent/manifest", get(handle_agent_manifest))
        .route("/system/status", get(handle_system_status))
        .route("/system/discord-config", post(handle_save_discord_config))
}

fn handle_agent_manifest(
    State(ctx): State<AppContext>,
) -> Ready<(StatusCode, Json<AgentManifestResponse>)> {
    ready((StatusCode::OK, Json(get_agent_manifest(&ctx))))
}

async fn handle_system_status(
    State(ctx): State<AppContext>,
    _owner: AuthenticatedOwner,
) -> Result<(StatusCode, Json<DesktopSystemStatusResponse>), ApiError> {
    let resp = get_desktop_system_status(&ctx).await?;
    Ok((StatusCode::OK, Json(resp)))
}

async fn handle_save_discord_config(
    State(ctx): State<AppContext>,
    _owner: AuthenticatedOwner,
    payload: Result<Json<SaveDiscordConfigRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<SaveDiscordConfigResponse>), ApiError> {
    let Json(req) = payload.map_err(|rej| ApiError::BadRequest(rej.body_text()))?;
    let resp = save_discord_config(&ctx, req).await?;
    Ok((StatusCode::OK, Json(resp)))
}
