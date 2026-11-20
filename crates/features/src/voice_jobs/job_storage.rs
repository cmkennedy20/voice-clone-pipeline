//! Dual-format (`Ogg`/`WAV`) disk writer, `VoiceJobReceipt` model, and `voice_jobs` `SQLx` writer.

use chrono::{DateTime, Utc};
use common::basis_points::BasisPoints;
use domain::translation::types::{TargetLanguage, TtsEngineKind};
use framework::context::AppContext;

use crate::{
    error::AppError,
    shared_queries::profile_loader::{VoiceJobId, VoiceProfileId, discord_id_to_i64},
};

/// Auditable ledger receipt returned and persisted upon verified voice job completion.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VoiceJobReceipt {
    pub job_id: VoiceJobId,
    pub profile_id: VoiceProfileId,
    pub profile_slug: String,
    pub target_language: TargetLanguage,
    pub delivery_mode: String,
    pub english_transcript: String,
    pub spoken_text: String,
    pub spoken_hanzi: String,
    pub phonetic_guide: String,
    pub pinyin: String,
    pub english_back_gloss: String,
    pub back_transcribed_text: String,
    pub back_transcribed_zh: String,
    pub engine_used: TtsEngineKind,
    pub cer_bps: BasisPoints,
    pub attempts_used: u8,
    pub duration_ms: u32,
    pub total_latency_ms: u32,
    pub output_ogg_path: String,
    pub discord_message_id: Option<u64>,
    pub created_at: DateTime<Utc>,
}

pub(crate) async fn write_dual_audio_files(
    ctx: &AppContext,
    job_id: VoiceJobId,
    ogg_bytes: &[u8],
    wav_bytes: &[u8],
) -> Result<(String, String), AppError> {
    let jobs_dir = ctx.config.storage_dir.join("voice_jobs");
    tokio::fs::create_dir_all(&jobs_dir)
        .await
        .map_err(|err| AppError::Internal(format!("failed creating voice_jobs dir: {err}")))?;

    let ogg_path = jobs_dir.join(format!("{job_id}.ogg"));
    tokio::fs::write(&ogg_path, ogg_bytes)
        .await
        .map_err(|err| AppError::Internal(format!("failed writing output Ogg file: {err}")))?;

    let wav_path = jobs_dir.join(format!("{job_id}.wav"));
    tokio::fs::write(&wav_path, wav_bytes)
        .await
        .map_err(|err| AppError::Internal(format!("failed writing output WAV file: {err}")))?;

    Ok((
        ogg_path.to_string_lossy().into_owned(),
        wav_path.to_string_lossy().into_owned(),
    ))
}

pub(crate) async fn persist_voice_job_row(
    db: &sqlx::PgPool,
    receipt: &VoiceJobReceipt,
    source_sha256: &str,
    output_wav_path: &str,
    discord_channel_id: Option<i64>,
) -> Result<(), AppError> {
    let cer_bps_i32 = i32::try_from(receipt.cer_bps.value()).unwrap_or(i32::MAX);
    let attempts_i16 = i16::from(receipt.attempts_used);
    let duration_i32 = i32::try_from(receipt.duration_ms).unwrap_or(i32::MAX);
    let latency_i32 = i32::try_from(receipt.total_latency_ms).unwrap_or(i32::MAX);
    let message_id_i64 = receipt
        .discord_message_id
        .map(|id| discord_id_to_i64(id, "discord_message_id"))
        .transpose()?;

    sqlx::query(
        "INSERT INTO voice_jobs (
            id, profile_id, target_language, delivery_mode, source_sha256, english_transcript,
            spoken_hanzi, pinyin, english_back_gloss, back_transcribed_zh,
            engine_used, cer_bps, attempts_used, duration_ms, total_latency_ms,
            output_ogg_path, output_wav_path, discord_channel_id, discord_message_id, created_at
         ) VALUES (
            $1, $2, $3, $4, $5, $6,
            $7, $8, $9, $10,
            $11, $12, $13, $14, $15,
            $16, $17, $18, $19, $20
         )",
    )
    .bind(receipt.job_id.as_uuid())
    .bind(receipt.profile_id.as_uuid())
    .bind(receipt.target_language.code())
    .bind(&receipt.delivery_mode)
    .bind(source_sha256)
    .bind(&receipt.english_transcript)
    .bind(&receipt.spoken_text)
    .bind(&receipt.phonetic_guide)
    .bind(&receipt.english_back_gloss)
    .bind(&receipt.back_transcribed_text)
    .bind(receipt.engine_used.as_str())
    .bind(cer_bps_i32)
    .bind(attempts_i16)
    .bind(duration_i32)
    .bind(latency_i32)
    .bind(&receipt.output_ogg_path)
    .bind(output_wav_path)
    .bind(discord_channel_id)
    .bind(message_id_i64)
    .bind(receipt.created_at)
    .execute(db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed inserting voice_jobs row"))?;

    Ok(())
}
