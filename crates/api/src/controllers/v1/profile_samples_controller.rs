//! `v1` multi-sample (`/{slug}/samples`) and `RTX 3090` fine-tuning (`/{slug}/finetune`) controllers.

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::Response,
    routing::{delete, get},
};
use features::{
    profiles::{
        finetune_profile::{
            StartProfileFinetuneInput, VoiceFinetuneJobRecord, get_profile_finetune_status,
            start_profile_finetune,
        },
        manage_samples::{
            AddProfileSampleInput, add_profile_sample, delete_profile_sample, list_profile_samples,
            read_profile_sample_audio,
        },
    },
    shared_queries::{
        profile_loader::VoiceProfileRecord,
        sample_queries::{VoiceProfileSampleRecord, VoiceSampleId},
    },
};
use framework::context::AppContext;

use crate::{error::ApiError, extractors::AuthenticatedOwner};

fn parse_voice_sample_id(raw_id: &str) -> Result<VoiceSampleId, ApiError> {
    raw_id
        .parse::<VoiceSampleId>()
        .map_err(|err| ApiError::BadRequest(format!("invalid sample id `{raw_id}`: {err}")))
}

/// Mounts multi-sample and fine-tuning routes nested under `/v1/profiles`.
pub fn routes() -> Router<AppContext> {
    Router::new()
        .route(
            "/{slug}/samples",
            get(get_samples_for_profile).post(post_sample_for_profile),
        )
        .route("/{slug}/samples/{sample_id}", delete(delete_sample_by_id))
        .route(
            "/{slug}/samples/{sample_id}/audio",
            get(get_sample_audio_by_id),
        )
        .route(
            "/{slug}/finetune",
            get(get_finetune_status).post(post_start_finetune),
        )
}

async fn get_samples_for_profile(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(slug): Path<String>,
) -> Result<(StatusCode, Json<Vec<VoiceProfileSampleRecord>>), ApiError> {
    let samples = list_profile_samples(&ctx, owner.discord_user_id, &slug).await?;
    Ok((StatusCode::OK, Json(samples)))
}

async fn post_sample_for_profile(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(slug): Path<String>,
    payload: Result<Json<AddProfileSampleInput>, JsonRejection>,
) -> Result<(StatusCode, Json<VoiceProfileRecord>), ApiError> {
    let Json(input) = payload.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let updated = add_profile_sample(&ctx, owner.discord_user_id, &slug, input).await?;
    Ok((StatusCode::CREATED, Json(updated)))
}

async fn get_sample_audio_by_id(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path((slug, raw_sample_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let sample_id = parse_voice_sample_id(&raw_sample_id)?;
    let wav_bytes =
        read_profile_sample_audio(&ctx, owner.discord_user_id, &slug, sample_id).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/wav")
        .body(Body::from(wav_bytes))
        .map_err(|err| ApiError::Internal(format!("response build error: {err}")))
}

async fn delete_sample_by_id(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path((slug, raw_sample_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<VoiceProfileRecord>), ApiError> {
    let sample_id = parse_voice_sample_id(&raw_sample_id)?;
    let updated = delete_profile_sample(&ctx, owner.discord_user_id, &slug, sample_id).await?;
    Ok((StatusCode::OK, Json(updated)))
}

async fn post_start_finetune(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(slug): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<VoiceFinetuneJobRecord>), ApiError> {
    let input = if body.iter().all(u8::is_ascii_whitespace) {
        StartProfileFinetuneInput::default()
    } else {
        serde_json::from_slice::<StartProfileFinetuneInput>(&body)
            .map_err(|err| ApiError::BadRequest(format!("invalid finetune JSON payload: {err}")))?
    };
    let job = start_profile_finetune(&ctx, owner.discord_user_id, &slug, input).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn get_finetune_status(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Path(slug): Path<String>,
) -> Result<(StatusCode, Json<Option<VoiceFinetuneJobRecord>>), ApiError> {
    let status = get_profile_finetune_status(&ctx, owner.discord_user_id, &slug).await?;
    Ok((StatusCode::OK, Json(status)))
}
