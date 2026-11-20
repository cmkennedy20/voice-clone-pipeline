//! Direct read projection (`get_voice_job`) and shared row mapper for persisted voice job receipts.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use common::basis_points::BasisPoints;
use domain::translation::types::{TargetLanguage, TtsEngineKind};
use framework::context::AppContext;
use uuid::Uuid;

use crate::{
    error::AppError,
    shared_queries::profile_loader::{
        VoiceJobId, VoiceProfileId, discord_id_from_i64, ensure_caller_is_owner,
    },
    voice_jobs::translate_clip::VoiceJobReceipt,
};

/// Read-side projection alias for [`VoiceJobReceipt`].
pub type VoiceJobView = VoiceJobReceipt;

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct VoiceJobJoinedRow {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub profile_slug: String,
    pub target_language: String,
    pub delivery_mode: String,
    pub english_transcript: String,
    pub spoken_hanzi: String,
    pub pinyin: String,
    pub english_back_gloss: String,
    pub back_transcribed_zh: String,
    pub engine_used: String,
    pub cer_bps: i32,
    pub attempts_used: i16,
    pub duration_ms: i32,
    pub total_latency_ms: i32,
    pub output_ogg_path: String,
    pub discord_message_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub owner_discord_user_id: i64,
}

pub(crate) fn map_joined_row_to_receipt(
    row: VoiceJobJoinedRow,
    caller_discord_user_id: u64,
) -> Result<VoiceJobReceipt, AppError> {
    let row_owner = discord_id_from_i64(row.owner_discord_user_id, "owner_discord_user_id")?;
    if row_owner != caller_discord_user_id {
        return Err(AppError::Forbidden(format!(
            "voice job '{}' is not owned by Discord user {caller_discord_user_id}",
            row.id
        )));
    }

    let target_language = TargetLanguage::from_str(&row.target_language)
        .map_err(|err| AppError::Internal(format!("stored target_language invalid: {err}")))?;
    let engine_used = TtsEngineKind::from_str(&row.engine_used)
        .map_err(|err| AppError::Internal(format!("stored engine_used invalid: {err}")))?;
    let cer_u32 = u32::try_from(row.cer_bps)
        .map_err(|_| AppError::Internal(format!("negative cer_bps stored: {}", row.cer_bps)))?;
    let attempts_used = u8::try_from(row.attempts_used).map_err(|_| {
        AppError::Internal(format!("out-of-range attempts_used: {}", row.attempts_used))
    })?;
    let duration_ms = u32::try_from(row.duration_ms).map_err(|_| {
        AppError::Internal(format!("negative duration_ms stored: {}", row.duration_ms))
    })?;
    let total_latency_ms = u32::try_from(row.total_latency_ms).map_err(|_| {
        AppError::Internal(format!(
            "negative total_latency_ms stored: {}",
            row.total_latency_ms
        ))
    })?;
    let discord_message_id = row
        .discord_message_id
        .map(|id| discord_id_from_i64(id, "discord_message_id"))
        .transpose()?;

    Ok(VoiceJobReceipt {
        job_id: VoiceJobId::from_uuid(row.id),
        profile_id: VoiceProfileId::from_uuid(row.profile_id),
        profile_slug: row.profile_slug,
        target_language,
        delivery_mode: row.delivery_mode,
        english_transcript: row.english_transcript,
        spoken_text: row.spoken_hanzi.clone(),
        spoken_hanzi: row.spoken_hanzi,
        phonetic_guide: row.pinyin.clone(),
        pinyin: row.pinyin,
        english_back_gloss: row.english_back_gloss,
        back_transcribed_text: row.back_transcribed_zh.clone(),
        back_transcribed_zh: row.back_transcribed_zh,
        engine_used,
        cer_bps: BasisPoints::from_raw(cer_u32),
        attempts_used,
        duration_ms,
        total_latency_ms,
        output_ogg_path: row.output_ogg_path,
        discord_message_id,
        created_at: row.created_at,
    })
}

/// Loads a persisted [`VoiceJobReceipt`] by `job_id`, enforcing owner authorization.
pub async fn get_voice_job(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    job_id: VoiceJobId,
) -> Result<VoiceJobReceipt, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;

    let row = sqlx::query_as::<_, VoiceJobJoinedRow>(
        "SELECT vj.id,
                vj.profile_id,
                vp.slug AS profile_slug,
                vj.target_language,
                vj.delivery_mode,
                vj.english_transcript,
                vj.spoken_hanzi,
                vj.pinyin,
                vj.english_back_gloss,
                vj.back_transcribed_zh,
                vj.engine_used,
                vj.cer_bps,
                vj.attempts_used,
                vj.duration_ms,
                vj.total_latency_ms,
                vj.output_ogg_path,
                vj.discord_message_id,
                vj.created_at,
                vp.owner_discord_user_id
           FROM voice_jobs vj
           INNER JOIN voice_profiles vp ON vp.id = vj.profile_id
          WHERE vj.id = $1",
    )
    .bind(job_id.as_uuid())
    .fetch_optional(&ctx.db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed querying voice_jobs by id"))?
    .ok_or_else(|| AppError::NotFound(format!("voice job '{job_id}' was not found")))?;

    map_joined_row_to_receipt(row, caller_discord_user_id)
}
