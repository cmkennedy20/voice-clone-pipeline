//! Thin `v1` runtime configuration (`GET /config`) and polyglot language catalog (`GET /languages`) controllers.

use std::future::{Ready, ready};

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use domain::translation::types::{LanguageMetadata, TargetLanguage};
use framework::context::AppContext;
use serde::Serialize;

use crate::extractors::AuthenticatedOwner;

/// Public runtime capabilities and ceiling parameters exposed to studio and CLI clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeConfigResponse {
    pub service_name: &'static str,
    pub discord_owner_user_id: u64,
    pub discord_owner_user_id_str: String,
    pub discord_application_id: u64,
    pub discord_application_id_str: String,
    pub discord_configured: bool,
    pub standalone_desktop_mode: bool,
    pub user_install_url: Option<String>,
    pub bot_dm_url: Option<String>,
    pub default_target_language: &'static str,
    pub max_cer_bps: u32,
    pub max_tts_attempts: u8,
    pub supported_engines: [&'static str; 2],
    pub supported_delivery_modes: [&'static str; 4],
}

/// Mounts `GET /config` and `GET /languages` under `/v1`.
pub fn routes() -> Router<AppContext> {
    Router::new()
        .route("/config", get(get_runtime_config))
        .route("/languages", get(get_supported_languages))
}

fn get_runtime_config(
    State(ctx): State<AppContext>,
    _owner: AuthenticatedOwner,
) -> Ready<(StatusCode, Json<RuntimeConfigResponse>)> {
    let oauth_pair = ctx.config.discord_oauth_urls();
    ready((
        StatusCode::OK,
        Json(RuntimeConfigResponse {
            service_name: "voice-clone-pipeline",
            discord_owner_user_id: ctx.config.discord_owner_user_id,
            discord_owner_user_id_str: ctx.config.discord_owner_user_id.to_string(),
            discord_application_id: ctx.config.discord_application_id,
            discord_application_id_str: ctx.config.discord_application_id.to_string(),
            discord_configured: ctx.config.is_discord_configured(),
            standalone_desktop_mode: ctx.config.desktop.standalone_mode,
            user_install_url: oauth_pair.as_ref().map(|(u, _)| u.clone()),
            bot_dm_url: oauth_pair.map(|(_, b)| b),
            default_target_language: "zh",
            max_cer_bps: ctx.config.max_cer_bps.value(),
            max_tts_attempts: ctx.config.max_tts_attempts,
            supported_engines: ["cosyvoice_3", "xtts_v2"],
            supported_delivery_modes: [
                "client_return_only",
                "user_install_followup",
                "profile_webhook",
                "bot_voice_bubble",
            ],
        }),
    ))
}

fn get_supported_languages() -> Ready<(StatusCode, Json<Vec<LanguageMetadata>>)> {
    ready((StatusCode::OK, Json(TargetLanguage::all_metadata())))
}
