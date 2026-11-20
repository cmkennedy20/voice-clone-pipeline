//! Multi-sample `SQLx` projections and hydration helpers for `voice_profile_samples`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    error::AppError,
    shared_queries::profile_loader::{VoiceProfileId, VoiceProfileRecord},
};

common::define_id!(VoiceSampleId);
common::define_id!(VoiceFinetuneJobId);

/// Persisted speaker reference sample row loaded from `voice_profile_samples`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VoiceProfileSampleRecord {
    pub id: VoiceSampleId,
    pub profile_id: VoiceProfileId,
    pub wav_path: String,
    pub transcript_en: String,
    pub style_label: String,
    pub duration_ms: u32,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct VoiceProfileSampleRow {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub wav_path: String,
    pub transcript_en: String,
    pub style_label: String,
    pub duration_ms: i32,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<VoiceProfileSampleRow> for VoiceProfileSampleRecord {
    type Error = AppError;

    fn try_from(row: VoiceProfileSampleRow) -> Result<Self, Self::Error> {
        let duration_ms = u32::try_from(row.duration_ms).map_err(|_| {
            AppError::Internal(format!(
                "negative duration_ms stored in voice_profile_samples: {}",
                row.duration_ms
            ))
        })?;
        Ok(Self {
            id: VoiceSampleId::from_uuid(row.id),
            profile_id: VoiceProfileId::from_uuid(row.profile_id),
            wav_path: row.wav_path,
            transcript_en: row.transcript_en,
            style_label: row.style_label,
            duration_ms,
            is_primary: row.is_primary,
            created_at: row.created_at,
        })
    }
}

/// Attaches ordered sample records and updates `sample_count` + `total_sample_duration_ms`.
pub(crate) fn hydrate_profile_samples(
    record: &mut VoiceProfileRecord,
    samples: Vec<VoiceProfileSampleRecord>,
) {
    record.sample_count = samples.len();
    record.total_sample_duration_ms = samples
        .iter()
        .fold(0_u32, |acc, item| acc.saturating_add(item.duration_ms));
    record.samples = samples;
}

/// Loads all `voice_profile_samples` rows for a single `profile_id`, primary-first then oldest-first.
pub(crate) async fn load_samples_for_profile(
    db: &sqlx::PgPool,
    profile_id: VoiceProfileId,
) -> Result<Vec<VoiceProfileSampleRecord>, AppError> {
    let rows = sqlx::query_as::<_, VoiceProfileSampleRow>(
        "SELECT id, profile_id, wav_path, transcript_en, style_label, duration_ms, is_primary, created_at
           FROM voice_profile_samples
          WHERE profile_id = $1
          ORDER BY is_primary DESC, created_at ASC, id ASC",
    )
    .bind(profile_id.as_uuid())
    .fetch_all(db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed querying voice_profile_samples"))?;

    let mut samples = Vec::with_capacity(rows.len());
    for row in rows {
        samples.push(VoiceProfileSampleRecord::try_from(row)?);
    }
    Ok(samples)
}

/// Batch-loads `voice_profile_samples` grouped by `profile_id` without `N+1` queries.
pub(crate) async fn load_samples_for_profiles(
    db: &sqlx::PgPool,
    profile_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<VoiceProfileSampleRecord>>, AppError> {
    if profile_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_as::<_, VoiceProfileSampleRow>(
        "SELECT id, profile_id, wav_path, transcript_en, style_label, duration_ms, is_primary, created_at
           FROM voice_profile_samples
          WHERE profile_id = ANY($1)
          ORDER BY profile_id ASC, is_primary DESC, created_at ASC, id ASC",
    )
    .bind(profile_ids)
    .fetch_all(db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed batch-loading voice_profile_samples"))?;

    let mut grouped: HashMap<Uuid, Vec<VoiceProfileSampleRecord>> =
        HashMap::with_capacity(profile_ids.len());
    for row in rows {
        let pid = row.profile_id;
        grouped
            .entry(pid)
            .or_default()
            .push(VoiceProfileSampleRecord::try_from(row)?);
    }
    Ok(grouped)
}

/// Inserts a newly enrolled `voice_profiles` row and its `voice_profile_samples` rows atomically.
pub(crate) async fn insert_enrolled_profile_and_samples(
    db: &sqlx::PgPool,
    record: &VoiceProfileRecord,
    owner_i64: i64,
    tuning_json: &str,
    samples: &[VoiceProfileSampleRecord],
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;

    sqlx::query(
        "INSERT INTO voice_profiles (
            id, slug, display_name, avatar_url, reference_wav_path,
            reference_transcript_en, preferred_engine, default_target_language,
            owner_discord_user_id, tuning_json, finetune_status, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb, 'none', $11)",
    )
    .bind(record.id.as_uuid())
    .bind(&record.slug)
    .bind(&record.display_name)
    .bind(&record.avatar_url)
    .bind(&record.reference_wav_path)
    .bind(&record.reference_transcript_en)
    .bind(record.preferred_engine.as_str())
    .bind(record.default_target_language.code())
    .bind(owner_i64)
    .bind(tuning_json)
    .bind(record.created_at)
    .execute(&mut *tx)
    .await?;

    for sample in samples {
        let duration_i32 = i32::try_from(sample.duration_ms).unwrap_or(i32::MAX);
        sqlx::query(
            "INSERT INTO voice_profile_samples (
                id, profile_id, wav_path, transcript_en, style_label,
                duration_ms, is_primary, created_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(sample.id.as_uuid())
        .bind(sample.profile_id.as_uuid())
        .bind(&sample.wav_path)
        .bind(&sample.transcript_en)
        .bind(&sample.style_label)
        .bind(duration_i32)
        .bind(sample.is_primary)
        .bind(sample.created_at)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await
}
