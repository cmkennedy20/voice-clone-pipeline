//! Thin `v1` voice job controllers (`GET /`, `POST /`, `POST /render`, `GET /{id}`, `GET /{id}/audio`).

use axum::{
    Json, Router,
    extract::{
        Multipart, Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use common::pagination::Paged;
use features::voice_jobs::{
    get_voice_job::get_voice_job,
    list_voice_jobs::{list_voice_jobs, read_voice_job_audio},
    translate_clip::{TranslateClipInput, VoiceJobReceipt, translate_clip},
};
use framework::context::AppContext;

pub use crate::render_io::{AudioReplayQuery, ListJobsQuery, RenderQuery};
use crate::{
    error::ApiError,
    extractors::AuthenticatedOwner,
    render_io::{
        RenderAudioFormat, build_list_jobs_filter, build_render_input_from_multipart,
        build_rendered_audio_response, build_replay_audio_response, parse_voice_job_id,
    },
};

/// Mounts `/`, `/render`, `/{id}`, and `/{id}/audio` under `/v1/voice-jobs`.
pub fn routes() -> Router<AppContext> {
    Router::new()
        .route("/", get(get_voice_jobs).post(post_voice_job))
        .route("/render", post(post_render_audio_for_user_client))
        .route("/{id}", get(get_voice_job_by_id))
        .route("/{id}/audio", get(get_voice_job_audio))
}

async fn get_voice_jobs(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    query: Result<Query<ListJobsQuery>, QueryRejection>,
) -> Result<(StatusCode, Json<Paged<VoiceJobReceipt>>), ApiError> {
    let Query(q) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let filter = build_list_jobs_filter(q)?;
    let paged = list_voice_jobs(&ctx, owner.discord_user_id, filter).await?;
    Ok((StatusCode::OK, Json(paged)))
}

async fn post_voice_job(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    payload: Result<Json<TranslateClipInput>, JsonRejection>,
) -> Result<(StatusCode, Json<VoiceJobReceipt>), ApiError> {
    let Json(body) = payload.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let out = translate_clip(&ctx, owner.discord_user_id, body).await?;
    Ok((StatusCode::CREATED, Json(out.receipt)))
}

async fn post_render_audio_for_user_client(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    query: Result<Query<RenderQuery>, QueryRejection>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let Query(q) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let (format, input) = build_render_input_from_multipart(q, &mut multipart).await?;
    let out = translate_clip(&ctx, owner.discord_user_id, input).await?;
    build_rendered_audio_response(format, out)
}

async fn get_voice_job_by_id(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(raw_id): Path<String>,
) -> Result<(StatusCode, Json<VoiceJobReceipt>), ApiError> {
    let job_id = parse_voice_job_id(&raw_id)?;
    let receipt = get_voice_job(&ctx, owner.discord_user_id, job_id).await?;
    Ok((StatusCode::OK, Json(receipt)))
}

async fn get_voice_job_audio(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(raw_id): Path<String>,
    query: Result<Query<AudioReplayQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let format = RenderAudioFormat::parse(q.format.as_deref())?;
    let job_id = parse_voice_job_id(&raw_id)?;
    let audio_bytes =
        read_voice_job_audio(&ctx, owner.discord_user_id, job_id, format.is_wav()).await?;
    build_replay_audio_response(format, audio_bytes)
}
