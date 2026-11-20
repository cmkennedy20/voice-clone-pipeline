//! Typed entity identifiers, `VoiceProfileRecord` projection, and shared authorization/query helpers.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use common::digest::sha256_hex;
use domain::{
    ports::{ReferenceSampleRef, SynthesisTuning},
    translation::types::{TargetLanguage, TtsEngineKind},
};
use framework::context::AppContext;
use uuid::Uuid;

use crate::{
    error::AppError,
    shared_queries::sample_queries::{
        VoiceProfileSampleRecord, hydrate_profile_samples, load_samples_for_profile,
    },
};

common::define_id!(VoiceProfileId);
common::define_id!(VoiceJobId);
common::define_id!(BenchmarkRunId);

fn default_finetune_status() -> String {
    "none".to_owned()
}

/// Persisted speaker profile metadata loaded from `voice_profiles` with hydrated multi-sample clips.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VoiceProfileRecord {
    pub id: VoiceProfileId,
    pub slug: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub reference_wav_path: String,
    pub reference_transcript_en: String,
    pub preferred_engine: TtsEngineKind,
    #[serde(default)]
    pub default_target_language: TargetLanguage,
    pub owner_discord_user_id: u64,
    #[serde(default)]
    pub tuning: SynthesisTuning,
    #[serde(default = "default_finetune_status")]
    pub finetune_status: String,
    #[serde(default)]
    pub finetuned_checkpoint_dir: Option<String>,
    #[serde(default)]
    pub finetune_error: Option<String>,
    #[serde(default)]
    pub finetuned_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub samples: Vec<VoiceProfileSampleRecord>,
    #[serde(default)]
    pub sample_count: usize,
    #[serde(default)]
    pub total_sample_duration_ms: u32,
    pub created_at: DateTime<Utc>,
}

impl VoiceProfileRecord {
    /// Builds ordered [`ReferenceSampleRef`] entries from enrolled samples, falling back to
    /// `reference_wav_path` and `reference_transcript_en` when `samples` is empty.
    #[must_use]
    pub fn reference_sample_refs(&self) -> Vec<ReferenceSampleRef> {
        if self.samples.is_empty() {
            return vec![ReferenceSampleRef {
                wav_path: self.reference_wav_path.clone(),
                transcript_en: self.reference_transcript_en.clone(),
            }];
        }
        self.samples
            .iter()
            .map(|sample| ReferenceSampleRef {
                wav_path: sample.wav_path.clone(),
                transcript_en: sample.transcript_en.clone(),
            })
            .collect()
    }
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct VoiceProfileRow {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub reference_wav_path: String,
    pub reference_transcript_en: String,
    pub preferred_engine: String,
    pub default_target_language: String,
    pub owner_discord_user_id: i64,
    pub tuning_json: String,
    pub finetune_status: String,
    pub finetuned_checkpoint_dir: Option<String>,
    pub finetune_error: Option<String>,
    pub finetuned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<VoiceProfileRow> for VoiceProfileRecord {
    type Error = AppError;

    fn try_from(row: VoiceProfileRow) -> Result<Self, Self::Error> {
        let preferred_engine = TtsEngineKind::from_str(&row.preferred_engine)
            .map_err(|err| AppError::Internal(format!("stored preferred_engine invalid: {err}")))?;
        let default_target_language = TargetLanguage::from_str(&row.default_target_language)
            .map_err(|err| {
                AppError::Internal(format!("stored default_target_language invalid: {err}"))
            })?;
        let owner_discord_user_id =
            discord_id_from_i64(row.owner_discord_user_id, "owner_discord_user_id")?;
        let tuning = serde_json::from_str::<SynthesisTuning>(&row.tuning_json)
            .map_err(|err| AppError::Internal(format!("stored tuning_json invalid: {err}")))?;

        Ok(Self {
            id: VoiceProfileId::from_uuid(row.id),
            slug: row.slug,
            display_name: row.display_name,
            avatar_url: row.avatar_url,
            reference_wav_path: row.reference_wav_path,
            reference_transcript_en: row.reference_transcript_en,
            preferred_engine,
            default_target_language,
            owner_discord_user_id,
            tuning,
            finetune_status: row.finetune_status,
            finetuned_checkpoint_dir: row.finetuned_checkpoint_dir,
            finetune_error: row.finetune_error,
            finetuned_at: row.finetuned_at,
            samples: Vec::new(),
            sample_count: 0,
            total_sample_duration_ms: 0,
            created_at: row.created_at,
        })
    }
}

pub(crate) fn require_trimmed_non_empty(raw: &str, field: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(format!("{field} cannot be empty")));
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn ensure_caller_is_owner(
    ctx: &AppContext,
    caller_discord_user_id: u64,
) -> Result<(), AppError> {
    if caller_discord_user_id != ctx.config.discord_owner_user_id {
        return Err(AppError::Forbidden(format!(
            "Discord user {caller_discord_user_id} is not authorized owner"
        )));
    }
    Ok(())
}

pub(crate) fn discord_id_to_i64(id: u64, field: &str) -> Result<i64, AppError> {
    i64::try_from(id)
        .map_err(|_| AppError::Validation(format!("{field} exceeds signed 64-bit integer range")))
}

pub(crate) fn discord_id_from_i64(id: i64, field: &str) -> Result<u64, AppError> {
    u64::try_from(id)
        .map_err(|_| AppError::Internal(format!("negative {field} stored in database: {id}")))
}

pub(crate) fn now_utc_micros(ctx: &AppContext) -> DateTime<Utc> {
    let raw = ctx.clock.now_utc();
    DateTime::from_timestamp_micros(raw.timestamp_micros()).unwrap_or(raw)
}

pub(crate) fn deterministic_attempt_seed(seed_material: &str, attempt: u8) -> u64 {
    let hex = sha256_hex(seed_material.as_bytes());
    let prefix = hex.get(..16).unwrap_or("000000000000002a");
    let base_seed = u64::from_str_radix(prefix, 16).unwrap_or(42);
    base_seed.wrapping_add(u64::from(attempt).wrapping_mul(1_000_003))
}

/// Loads a `voice_profiles` row by `slug`, verifies ownership, and hydrates its `samples`.
pub async fn load_owned_profile_by_slug(
    db: &sqlx::PgPool,
    slug: &str,
    caller_discord_user_id: u64,
) -> Result<VoiceProfileRecord, AppError> {
    let row = sqlx::query_as::<_, VoiceProfileRow>(
        "SELECT id, slug, display_name, avatar_url, reference_wav_path, reference_transcript_en,
                preferred_engine, default_target_language, owner_discord_user_id,
                tuning_json::text AS tuning_json, finetune_status, finetuned_checkpoint_dir,
                finetune_error, finetuned_at, created_at
           FROM voice_profiles
          WHERE slug = $1",
    )
    .bind(slug)
    .fetch_optional(db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed querying voice_profiles by slug"))?
    .ok_or_else(|| AppError::NotFound(format!("voice profile '{slug}' was not found")))?;

    let mut record = VoiceProfileRecord::try_from(row)?;
    if record.owner_discord_user_id != caller_discord_user_id {
        return Err(AppError::Forbidden(format!(
            "voice profile '{slug}' is not owned by Discord user {caller_discord_user_id}"
        )));
    }

    let samples = load_samples_for_profile(db, record.id).await?;
    hydrate_profile_samples(&mut record, samples);
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::{
        BenchmarkRunId, VoiceJobId, VoiceProfileId, deterministic_attempt_seed,
        discord_id_from_i64, discord_id_to_i64,
    };
    use crate::error::AppError;

    #[test]
    fn typed_ids_generate_v7_and_attempt_seeds_shift_monotonically() {
        assert_eq!(VoiceProfileId::new_v7().as_uuid().get_version_num(), 7);
        assert_eq!(VoiceJobId::new_v7().as_uuid().get_version_num(), 7);
        assert_eq!(BenchmarkRunId::new_v7().as_uuid().get_version_num(), 7);

        let s1 = deterministic_attempt_seed("clip-hash-abc", 1);
        let s2 = deterministic_attempt_seed("clip-hash-abc", 2);
        let s3 = deterministic_attempt_seed("clip-hash-abc", 3);
        assert_eq!(s2.wrapping_sub(s1), 1_000_003);
        assert_eq!(s3.wrapping_sub(s2), 1_000_003);
    }

    #[test]
    fn discord_id_conversions_enforce_non_negative_signed_64_bit_bounds() {
        assert_eq!(
            discord_id_to_i64(777_888_999, "owner").unwrap(),
            777_888_999
        );
        assert!(matches!(
            discord_id_to_i64(u64::MAX, "owner"),
            Err(AppError::Validation(_))
        ));
        assert_eq!(
            discord_id_from_i64(777_888_999, "owner").unwrap(),
            777_888_999
        );
        assert!(matches!(
            discord_id_from_i64(-1, "owner"),
            Err(AppError::Internal(_))
        ));
    }
}
