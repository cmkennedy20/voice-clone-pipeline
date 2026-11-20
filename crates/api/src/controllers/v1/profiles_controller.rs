//! Thin `v1` voice profile `CRUD` and reference-audio playback controllers.

use axum::{
    Json, Router,
    body::Body,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{StatusCode, header},
    response::Response,
    routing::{get, patch, post},
};
use common::pagination::{PageParams, Paged};
use features::{
    profiles::{
        delete_profile::delete_profile,
        enroll_profile::{EnrollProfileInput, enroll_profile},
        list_profiles::list_profiles,
        read_reference_audio::read_profile_reference_audio,
        update_profile::{UpdateProfileInput, update_profile},
    },
    shared_queries::profile_loader::VoiceProfileRecord,
};
use framework::context::AppContext;
use serde::Deserialize;

use crate::{
    controllers::v1::profile_samples_controller, error::ApiError, extractors::AuthenticatedOwner,
};

/// Optional query parameters for `GET /v1/profiles`.
#[derive(Debug, Default, Deserialize)]
pub struct PageQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Mounts `/`, `/{slug}`, `/{slug}/reference-audio`, `/{slug}/samples`, and `/{slug}/finetune` under `/v1/profiles`.
pub fn routes() -> Router<AppContext> {
    Router::new()
        .route("/", post(post_enroll_profile).get(get_profiles))
        .route(
            "/{slug}",
            patch(patch_profile).delete(delete_profile_by_slug),
        )
        .route("/{slug}/reference-audio", get(get_profile_reference_audio))
        .merge(profile_samples_controller::routes())
}

async fn post_enroll_profile(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    payload: Result<Json<EnrollProfileInput>, JsonRejection>,
) -> Result<(StatusCode, Json<VoiceProfileRecord>), ApiError> {
    let Json(input) = payload.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let profile = enroll_profile(&ctx, owner.discord_user_id, input).await?;
    Ok((StatusCode::CREATED, Json(profile)))
}

async fn get_profiles(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Result<(StatusCode, Json<Paged<VoiceProfileRecord>>), ApiError> {
    let Query(q) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let page = PageParams::new(
        q.limit.unwrap_or(PageParams::DEFAULT_LIMIT),
        q.offset.unwrap_or(0),
    );
    let profiles = list_profiles(&ctx, owner.discord_user_id, page).await?;
    Ok((StatusCode::OK, Json(profiles)))
}

async fn patch_profile(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(slug): Path<String>,
    payload: Result<Json<UpdateProfileInput>, JsonRejection>,
) -> Result<(StatusCode, Json<VoiceProfileRecord>), ApiError> {
    let Json(input) = payload.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let updated = update_profile(&ctx, owner.discord_user_id, &slug, input).await?;
    Ok((StatusCode::OK, Json(updated)))
}

async fn delete_profile_by_slug(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(slug): Path<String>,
) -> Result<StatusCode, ApiError> {
    delete_profile(&ctx, owner.discord_user_id, &slug).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_profile_reference_audio(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let wav_bytes = read_profile_reference_audio(&ctx, owner.discord_user_id, &slug).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/wav")
        .body(Body::from(wav_bytes))
        .map_err(|err| ApiError::Internal(format!("response build error: {err}")))
}
