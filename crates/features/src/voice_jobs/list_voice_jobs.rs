//! Paginated voice job history (`list_voice_jobs`) and stored audio replay (`read_voice_job_audio`).

use std::{io::ErrorKind, path::PathBuf};

use common::pagination::{PageParams, Paged};
use domain::translation::types::TargetLanguage;
use framework::context::AppContext;

use crate::{
    error::AppError,
    shared_queries::profile_loader::{
        VoiceJobId, discord_id_from_i64, discord_id_to_i64, ensure_caller_is_owner,
    },
    voice_jobs::{
        get_voice_job::{VoiceJobJoinedRow, map_joined_row_to_receipt},
        translate_clip::VoiceJobReceipt,
    },
};

/// Filter and pagination parameters for [`list_voice_jobs`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListVoiceJobsFilter {
    pub target_language: Option<TargetLanguage>,
    pub profile_slug: Option<String>,
    pub page: PageParams,
}

/// Lists persisted [`VoiceJobReceipt`] rows owned by `caller_discord_user_id` ordered newest-first.
pub async fn list_voice_jobs(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    filter: ListVoiceJobsFilter,
) -> Result<Paged<VoiceJobReceipt>, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let owner_i64 = discord_id_to_i64(caller_discord_user_id, "caller_discord_user_id")?;
    let clamped_page = PageParams::new(filter.page.limit, filter.page.offset);
    let limit_i64 = i64::from(clamped_page.limit);
    let offset_i64 = i64::from(clamped_page.offset);
    let lang_code = filter.target_language.map(TargetLanguage::code);
    let slug_filter = filter
        .profile_slug
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    let (total_i64,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)
           FROM voice_jobs vj
           INNER JOIN voice_profiles vp ON vp.id = vj.profile_id
          WHERE vp.owner_discord_user_id = $1
            AND ($2::text IS NULL OR vj.target_language = $2)
            AND ($3::text IS NULL OR vp.slug = $3)",
    )
    .bind(owner_i64)
    .bind(lang_code)
    .bind(&slug_filter)
    .fetch_one(&ctx.db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed counting voice_jobs"))?;

    let total = u64::try_from(total_i64).unwrap_or(0);
    let rows = sqlx::query_as::<_, VoiceJobJoinedRow>(
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
          WHERE vp.owner_discord_user_id = $1
            AND ($2::text IS NULL OR vj.target_language = $2)
            AND ($3::text IS NULL OR vp.slug = $3)
          ORDER BY vj.created_at DESC, vj.id DESC
          LIMIT $4 OFFSET $5",
    )
    .bind(owner_i64)
    .bind(lang_code)
    .bind(&slug_filter)
    .bind(limit_i64)
    .bind(offset_i64)
    .fetch_all(&ctx.db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed listing voice_jobs"))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(map_joined_row_to_receipt(row, caller_discord_user_id)?);
    }

    Ok(Paged::new(items, total, clamped_page))
}

#[derive(Debug, sqlx::FromRow)]
struct JobAudioPathRow {
    output_ogg_path: String,
    output_wav_path: Option<String>,
    owner_discord_user_id: i64,
}

/// Reads stored `.ogg` or `48 kHz` `.wav` bytes for an owned `voice_jobs` row.
pub async fn read_voice_job_audio(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    job_id: VoiceJobId,
    format_wav: bool,
) -> Result<Vec<u8>, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;

    let row = sqlx::query_as::<_, JobAudioPathRow>(
        "SELECT vj.output_ogg_path,
                vj.output_wav_path,
                vp.owner_discord_user_id
           FROM voice_jobs vj
           INNER JOIN voice_profiles vp ON vp.id = vj.profile_id
          WHERE vj.id = $1",
    )
    .bind(job_id.as_uuid())
    .fetch_optional(&ctx.db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed querying voice_jobs audio paths"))?
    .ok_or_else(|| AppError::NotFound(format!("voice job '{job_id}' was not found")))?;

    let row_owner = discord_id_from_i64(row.owner_discord_user_id, "owner_discord_user_id")?;
    if row_owner != caller_discord_user_id {
        return Err(AppError::Forbidden(format!(
            "voice job '{job_id}' is not owned by Discord user {caller_discord_user_id}"
        )));
    }

    let target_path = if format_wav {
        row.output_wav_path.map_or_else(
            || PathBuf::from(&row.output_ogg_path).with_extension("wav"),
            PathBuf::from,
        )
    } else {
        PathBuf::from(&row.output_ogg_path)
    };

    tokio::fs::read(&target_path)
        .await
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => AppError::NotFound(format!(
                "stored audio file for voice job '{job_id}' was not found"
            )),
            _ => AppError::Internal(format!(
                "failed reading audio file for voice job '{job_id}': {err}"
            )),
        })
}
