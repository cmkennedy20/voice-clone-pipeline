//! Reference voice enrollment workflow (`enroll_profile`) with `Gate 0` hygiene and multi-sample persistence.

use std::path::PathBuf;

use domain::{
    audio::gate::evaluate_audio_gate,
    ports::SynthesisTuning,
    translation::types::{TargetLanguage, TtsEngineKind},
};
use framework::{context::AppContext, db::pg_errors::is_unique_violation};

use crate::{
    error::AppError,
    shared_queries::{
        profile_loader::{
            VoiceProfileId, VoiceProfileRecord, discord_id_to_i64, ensure_caller_is_owner,
            now_utc_micros, require_trimmed_non_empty,
        },
        sample_queries::{
            VoiceProfileSampleRecord, VoiceSampleId, hydrate_profile_samples,
            insert_enrolled_profile_and_samples,
        },
    },
};

fn default_style_label() -> String {
    "conversational".to_owned()
}

pub(crate) fn normalize_style_label(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        default_style_label()
    } else {
        trimmed.to_owned()
    }
}

/// Secondary reference clip attached at enrollment time for multi-sample voice conditioning.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdditionalSampleInput {
    #[serde(with = "crate::shared_queries::bytes_serde")]
    pub audio_bytes: Vec<u8>,
    pub transcript_en: String,
    #[serde(default = "default_style_label")]
    pub style_label: String,
}

/// Input payload for enrolling a speaker voice profile, optional multi-sample clips, and tuning.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnrollProfileInput {
    pub slug: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    #[serde(with = "crate::shared_queries::bytes_serde")]
    pub reference_audio_bytes: Vec<u8>,
    pub reference_transcript_en: String,
    pub preferred_engine: TtsEngineKind,
    #[serde(default)]
    pub default_target_language: TargetLanguage,
    #[serde(default)]
    pub additional_samples: Vec<AdditionalSampleInput>,
    #[serde(default)]
    pub tuning: Option<SynthesisTuning>,
}

struct ValidatedAdditionalSample {
    id: VoiceSampleId,
    audio_bytes: Vec<u8>,
    transcript_en: String,
    style_label: String,
    duration_ms: u32,
    wav_path: PathBuf,
}

fn validate_profile_slug(slug: &str) -> Result<(), AppError> {
    let bytes = slug.as_bytes();
    let len = bytes.len();
    if !(2..=64).contains(&len) {
        return Err(AppError::Validation(format!(
            "profile slug length {len} must be within 2..=64 characters"
        )));
    }
    let Some((&first, rest)) = bytes.split_first() else {
        return Err(AppError::Validation(
            "profile slug cannot be empty".to_owned(),
        ));
    };
    if !first.is_ascii_lowercase() {
        return Err(AppError::Validation(
            "profile slug must start with an ASCII lowercase letter ('a'..='z')".to_owned(),
        ));
    }
    if !rest
        .iter()
        .all(|&b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
    {
        return Err(AppError::Validation(
            "profile slug may only contain ASCII lowercase letters, digits, '_', or '-'".to_owned(),
        ));
    }
    Ok(())
}

fn validate_additional_samples(
    ctx: &AppContext,
    slug_samples_dir: &std::path::Path,
    additional_samples: Vec<AdditionalSampleInput>,
) -> Result<Vec<ValidatedAdditionalSample>, AppError> {
    let mut prepared = Vec::with_capacity(additional_samples.len());
    for (idx, extra) in additional_samples.into_iter().enumerate() {
        let extra_transcript = require_trimmed_non_empty(
            &extra.transcript_en,
            &format!("additional_samples[{idx}].transcript_en"),
        )?;
        let (_extra_pcm, extra_metrics) = ctx
            .codec
            .decode_and_measure(&extra.audio_bytes)
            .map_err(|err| {
                AppError::Validation(format!("invalid additional sample #{idx} audio: {err}"))
            })?;
        evaluate_audio_gate(&extra_metrics)?;
        let sample_id = VoiceSampleId::new_v7();
        prepared.push(ValidatedAdditionalSample {
            id: sample_id,
            audio_bytes: extra.audio_bytes,
            transcript_en: extra_transcript,
            style_label: normalize_style_label(&extra.style_label),
            duration_ms: extra_metrics.total_duration_ms.max(1),
            wav_path: slug_samples_dir.join(format!("sample_{sample_id}.wav")),
        });
    }
    Ok(prepared)
}

fn build_enrolled_sample_records(
    profile_id: VoiceProfileId,
    primary_wav_str: String,
    reference_transcript_en: String,
    primary_duration_ms: u32,
    created_at: chrono::DateTime<chrono::Utc>,
    prepared_extras: &[ValidatedAdditionalSample],
) -> Vec<VoiceProfileSampleRecord> {
    let mut records = Vec::with_capacity(1 + prepared_extras.len());
    records.push(VoiceProfileSampleRecord {
        id: VoiceSampleId::from_uuid(profile_id.as_uuid()),
        profile_id,
        wav_path: primary_wav_str,
        transcript_en: reference_transcript_en,
        style_label: "primary".to_owned(),
        duration_ms: primary_duration_ms,
        is_primary: true,
        created_at,
    });
    for extra in prepared_extras {
        records.push(VoiceProfileSampleRecord {
            id: extra.id,
            profile_id,
            wav_path: extra.wav_path.to_string_lossy().into_owned(),
            transcript_en: extra.transcript_en.clone(),
            style_label: extra.style_label.clone(),
            duration_ms: extra.duration_ms,
            is_primary: false,
            created_at,
        });
    }
    records
}

async fn finalize_enrolled_files(
    staging_wav_path: &std::path::Path,
    final_wav_path: &std::path::Path,
    slug_samples_dir: &std::path::Path,
    prepared_extras: &[ValidatedAdditionalSample],
) -> Result<(), AppError> {
    tokio::fs::rename(staging_wav_path, final_wav_path)
        .await
        .map_err(|err| AppError::Internal(format!("failed finalizing reference WAV: {err}")))?;

    if !prepared_extras.is_empty() {
        tokio::fs::create_dir_all(slug_samples_dir)
            .await
            .map_err(|err| {
                AppError::Internal(format!("failed creating profile sample directory: {err}"))
            })?;
        for extra in prepared_extras {
            tokio::fs::write(&extra.wav_path, &extra.audio_bytes)
                .await
                .map_err(|err| {
                    AppError::Internal(format!("failed writing additional sample WAV: {err}"))
                })?;
        }
    }
    Ok(())
}

async fn commit_profile_and_write_wavs(
    ctx: &AppContext,
    record: &VoiceProfileRecord,
    owner_i64: i64,
    tuning_json: &str,
    sample_records: &[VoiceProfileSampleRecord],
    reference_audio_bytes: &[u8],
    prepared_extras: &[ValidatedAdditionalSample],
) -> Result<(), AppError> {
    let profiles_dir = ctx.config.storage_dir.join("voice_profiles");
    let slug_samples_dir = profiles_dir.join(&record.slug);
    tokio::fs::create_dir_all(&profiles_dir)
        .await
        .map_err(|err| {
            AppError::Internal(format!("failed creating voice_profiles directory: {err}"))
        })?;

    let final_wav_path = profiles_dir.join(format!("{}.wav", record.slug));
    let staging_wav_path = profiles_dir.join(format!(".{}.{}.tmp.wav", record.slug, record.id));
    tokio::fs::write(&staging_wav_path, reference_audio_bytes)
        .await
        .map_err(|err| AppError::Internal(format!("failed staging reference WAV: {err}")))?;

    if let Err(err) =
        insert_enrolled_profile_and_samples(&ctx.db, record, owner_i64, tuning_json, sample_records)
            .await
    {
        let _ = tokio::fs::remove_file(&staging_wav_path).await;
        if is_unique_violation(&err) {
            return Err(AppError::Conflict(format!(
                "voice profile slug '{}' already exists",
                record.slug
            )));
        }
        return Err(AppError::Internal(format!(
            "failed inserting voice profile '{}': {err}",
            record.slug
        )));
    }

    finalize_enrolled_files(
        &staging_wav_path,
        &final_wav_path,
        &slug_samples_dir,
        prepared_extras,
    )
    .await
}

/// Validates and enrolls a speaker profile, primary reference `WAV`, and optional extra samples.
pub async fn enroll_profile(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    input: EnrollProfileInput,
) -> Result<VoiceProfileRecord, AppError> {
    let EnrollProfileInput {
        slug,
        display_name,
        avatar_url,
        reference_audio_bytes,
        reference_transcript_en,
        preferred_engine,
        default_target_language,
        additional_samples,
        tuning,
    } = input;

    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    validate_profile_slug(&slug)?;
    let display_name = require_trimmed_non_empty(&display_name, "display_name")?;
    let reference_transcript_en =
        require_trimmed_non_empty(&reference_transcript_en, "reference_transcript_en")?;
    let avatar_url = avatar_url
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty());

    let resolved_tuning = tuning.unwrap_or_default();
    resolved_tuning.validate().map_err(AppError::Validation)?;
    let tuning_json = serde_json::to_string(&resolved_tuning)
        .map_err(|err| AppError::Internal(format!("failed serializing tuning JSON: {err}")))?;

    let (_pcm_16k, primary_metrics) = ctx
        .codec
        .decode_and_measure(&reference_audio_bytes)
        .map_err(|err| AppError::Validation(format!("invalid reference audio: {err}")))?;
    evaluate_audio_gate(&primary_metrics)?;
    let primary_duration_ms = primary_metrics.total_duration_ms.max(1);

    let profiles_dir = ctx.config.storage_dir.join("voice_profiles");
    let slug_samples_dir = profiles_dir.join(&slug);
    let prepared_extras = validate_additional_samples(ctx, &slug_samples_dir, additional_samples)?;

    let profile_id = VoiceProfileId::new_v7();
    let owner_i64 = discord_id_to_i64(caller_discord_user_id, "caller_discord_user_id")?;
    let created_at = now_utc_micros(ctx);
    let primary_wav_str = profiles_dir
        .join(format!("{slug}.wav"))
        .to_string_lossy()
        .into_owned();
    let sample_records = build_enrolled_sample_records(
        profile_id,
        primary_wav_str.clone(),
        reference_transcript_en.clone(),
        primary_duration_ms,
        created_at,
        &prepared_extras,
    );

    let mut record = VoiceProfileRecord {
        id: profile_id,
        slug,
        display_name,
        avatar_url,
        reference_wav_path: primary_wav_str,
        reference_transcript_en,
        preferred_engine,
        default_target_language,
        owner_discord_user_id: caller_discord_user_id,
        tuning: resolved_tuning,
        finetune_status: "none".to_owned(),
        finetuned_checkpoint_dir: None,
        finetune_error: None,
        finetuned_at: None,
        samples: Vec::new(),
        sample_count: 0,
        total_sample_duration_ms: 0,
        created_at,
    };

    commit_profile_and_write_wavs(
        ctx,
        &record,
        owner_i64,
        &tuning_json,
        &sample_records,
        &reference_audio_bytes,
        &prepared_extras,
    )
    .await?;

    hydrate_profile_samples(&mut record, sample_records);
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::validate_profile_slug;
    use crate::{error::AppError, shared_queries::profile_loader::require_trimmed_non_empty};

    #[test]
    fn slug_validator_accepts_canonical_slugs_and_rejects_invalid_boundaries() {
        assert_eq!(validate_profile_slug("ab"), Ok(()));
        assert_eq!(validate_profile_slug("default_voice_01"), Ok(()));
        assert_eq!(validate_profile_slug("default-voice"), Ok(()));
        assert_eq!(validate_profile_slug(&"a".repeat(64)), Ok(()));

        for invalid in [
            "",
            "a",
            &"a".repeat(65),
            "1voice",
            "_voice",
            "-voice",
            "Alex",
            "../voice",
            "voice/wav",
        ] {
            assert!(
                matches!(validate_profile_slug(invalid), Err(AppError::Validation(_))),
                "expected '{invalid}' to fail slug validation"
            );
        }
    }

    #[test]
    fn trimmed_text_validator_rejects_whitespace_only_strings() {
        assert_eq!(
            require_trimmed_non_empty("  Alex  ", "display_name").unwrap(),
            "Alex"
        );
        assert!(matches!(
            require_trimmed_non_empty("   \t\n ", "display_name"),
            Err(AppError::Validation(_))
        ));
    }
}
