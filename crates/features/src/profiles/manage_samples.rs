//! Multi-sample management workflows (`add_profile_sample`, `list_profile_samples`,
//! `read_profile_sample_audio`, `delete_profile_sample`).

use std::io::ErrorKind;

use domain::audio::gate::evaluate_audio_gate;
use framework::context::AppContext;

use crate::{
    error::AppError,
    profiles::enroll_profile::normalize_style_label,
    shared_queries::{
        profile_loader::{
            VoiceProfileRecord, ensure_caller_is_owner, load_owned_profile_by_slug, now_utc_micros,
            require_trimmed_non_empty,
        },
        sample_queries::{VoiceProfileSampleRecord, VoiceSampleId},
    },
};

fn default_style_label() -> String {
    "conversational".to_owned()
}

/// Input payload for appending a new reference clip to an enrolled voice profile.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddProfileSampleInput {
    #[serde(with = "crate::shared_queries::bytes_serde")]
    pub audio_bytes: Vec<u8>,
    pub transcript_en: String,
    #[serde(default = "default_style_label")]
    pub style_label: String,
    #[serde(default)]
    pub make_primary: bool,
}

/// Validates a new reference clip via `Gate 0`, writes `sample_{id}.wav`, inserts into
/// `voice_profile_samples`, optionally promotes it to primary, and returns the updated profile.
pub async fn add_profile_sample(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
    input: AddProfileSampleInput,
) -> Result<VoiceProfileRecord, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;
    let transcript_en = require_trimmed_non_empty(&input.transcript_en, "transcript_en")?;
    let style_label = normalize_style_label(&input.style_label);

    let (_pcm_16k, metrics) = ctx
        .codec
        .decode_and_measure(&input.audio_bytes)
        .map_err(|err| AppError::Validation(format!("invalid sample audio: {err}")))?;
    evaluate_audio_gate(&metrics)?;

    let duration_ms = metrics.total_duration_ms.max(1);
    let duration_i32 = i32::try_from(duration_ms).unwrap_or(i32::MAX);
    let sample_id = VoiceSampleId::new_v7();
    let created_at = now_utc_micros(ctx);

    let slug_dir = ctx.config.storage_dir.join("voice_profiles").join(slug);
    tokio::fs::create_dir_all(&slug_dir)
        .await
        .map_err(|err| AppError::Internal(format!("failed creating profile sample dir: {err}")))?;

    let wav_path = slug_dir.join(format!("sample_{sample_id}.wav"));
    tokio::fs::write(&wav_path, &input.audio_bytes)
        .await
        .map_err(|err| AppError::Internal(format!("failed writing sample WAV: {err}")))?;
    let wav_path_str = wav_path.to_string_lossy().into_owned();

    let is_primary = input.make_primary || profile.samples.is_empty();
    let mut tx = ctx
        .db
        .begin()
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed starting add sample transaction"))?;

    if is_primary {
        sqlx::query("UPDATE voice_profile_samples SET is_primary = FALSE WHERE profile_id = $1")
            .bind(profile.id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|err| AppError::from_sqlx(&err, "failed clearing previous primary sample"))?;

        sqlx::query(
            "UPDATE voice_profiles
                SET reference_wav_path = $1,
                    reference_transcript_en = $2
              WHERE id = $3",
        )
        .bind(&wav_path_str)
        .bind(&transcript_en)
        .bind(profile.id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed updating primary reference on profile"))?;
    }

    sqlx::query(
        "INSERT INTO voice_profile_samples (
            id, profile_id, wav_path, transcript_en, style_label,
            duration_ms, is_primary, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(sample_id.as_uuid())
    .bind(profile.id.as_uuid())
    .bind(&wav_path_str)
    .bind(&transcript_en)
    .bind(&style_label)
    .bind(duration_i32)
    .bind(is_primary)
    .bind(created_at)
    .execute(&mut *tx)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed inserting voice_profile_samples row"))?;

    tx.commit()
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed committing add sample transaction"))?;

    load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await
}

/// Lists all reference samples enrolled under `slug`, primary-first then oldest-first.
pub async fn list_profile_samples(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
) -> Result<Vec<VoiceProfileSampleRecord>, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;
    Ok(profile.samples)
}

/// Reads the raw `WAV` bytes for a specific `sample_id` belonging to `slug`.
pub async fn read_profile_sample_audio(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
    sample_id: VoiceSampleId,
) -> Result<Vec<u8>, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;
    let sample = profile
        .samples
        .iter()
        .find(|item| item.id == sample_id)
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "sample '{sample_id}' was not found on profile '{slug}'"
            ))
        })?;

    tokio::fs::read(&sample.wav_path)
        .await
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => AppError::NotFound(format!(
                "audio file for sample '{sample_id}' on profile '{slug}' was not found"
            )),
            _ => AppError::Internal(format!(
                "failed reading sample '{sample_id}' audio for profile '{slug}': {err}"
            )),
        })
}

/// Deletes `sample_id` from `slug`, preserving at least `1` sample and promoting the oldest
/// remaining sample to primary when the deleted clip was primary.
pub async fn delete_profile_sample(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
    sample_id: VoiceSampleId,
) -> Result<VoiceProfileRecord, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;

    let target = profile
        .samples
        .iter()
        .find(|item| item.id == sample_id)
        .cloned()
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "sample '{sample_id}' was not found on profile '{slug}'"
            ))
        })?;

    if profile.samples.len() <= 1 {
        return Err(AppError::Validation(format!(
            "cannot delete the final remaining sample on profile '{slug}'; a profile must retain at least 1 sample"
        )));
    }

    let mut tx =
        ctx.db.begin().await.map_err(|err| {
            AppError::from_sqlx(&err, "failed starting sample deletion transaction")
        })?;

    sqlx::query("DELETE FROM voice_profile_samples WHERE id = $1 AND profile_id = $2")
        .bind(sample_id.as_uuid())
        .bind(profile.id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed deleting voice_profile_samples row"))?;

    if target.is_primary {
        let replacement = profile
            .samples
            .iter()
            .filter(|item| item.id != sample_id)
            .min_by_key(|item| (item.created_at, item.id.as_uuid()))
            .ok_or_else(|| {
                AppError::Internal("no replacement sample available for primary promotion".into())
            })?;

        sqlx::query("UPDATE voice_profile_samples SET is_primary = TRUE WHERE id = $1")
            .bind(replacement.id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                AppError::from_sqlx(&err, "failed promoting replacement primary sample")
            })?;

        sqlx::query(
            "UPDATE voice_profiles
                SET reference_wav_path = $1,
                    reference_transcript_en = $2
              WHERE id = $3",
        )
        .bind(&replacement.wav_path)
        .bind(&replacement.transcript_en)
        .bind(profile.id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            AppError::from_sqlx(
                &err,
                "failed updating promoted primary reference on profile",
            )
        })?;
    }

    tx.commit()
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed committing sample deletion"))?;

    let _ = tokio::fs::remove_file(&target.wav_path).await;

    load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await
}
