//! Source preparation, `Gate 0` validation, synthesis tuning resolution, and multi-sample extraction.

use common::digest::sha256_hex;
use domain::{
    audio::{gate::evaluate_audio_gate, types::Pcm16kMono},
    ports::{ReferenceSampleRef, SynthesisTuning},
};
use framework::context::AppContext;

use crate::{error::AppError, shared_queries::profile_loader::VoiceProfileRecord};

const MAX_SOURCE_TEXT_CHARS: usize = 2_000;

pub(super) enum PreparedSource {
    Audio {
        pcm_16k: Pcm16kMono,
        sha256: String,
    },
    Text {
        english_transcript: String,
        sha256: String,
    },
}

impl PreparedSource {
    pub(super) fn sha256(&self) -> &str {
        match self {
            Self::Audio { sha256, .. } | Self::Text { sha256, .. } => sha256,
        }
    }
}

pub(super) fn prepare_input_source(
    ctx: &AppContext,
    audio_bytes: &[u8],
    source_text: Option<&str>,
) -> Result<PreparedSource, AppError> {
    if !audio_bytes.is_empty() {
        let (pcm_16k, metrics) = ctx
            .codec
            .decode_and_measure(audio_bytes)
            .map_err(|err| AppError::Validation(err.to_string()))?;
        evaluate_audio_gate(&metrics)?;
        return Ok(PreparedSource::Audio {
            pcm_16k,
            sha256: sha256_hex(audio_bytes),
        });
    }

    let trimmed = source_text.map_or("", str::trim);
    if trimmed.is_empty() {
        return Err(AppError::Validation(
            "provide either audio_bytes or source_text".to_owned(),
        ));
    }
    if trimmed.chars().count() > MAX_SOURCE_TEXT_CHARS {
        return Err(AppError::Validation(format!(
            "source_text cannot exceed {MAX_SOURCE_TEXT_CHARS} characters"
        )));
    }
    Ok(PreparedSource::Text {
        english_transcript: trimmed.to_owned(),
        sha256: sha256_hex(trimmed.as_bytes()),
    })
}

pub(super) fn resolve_synthesis_tuning_and_samples(
    profile: &VoiceProfileRecord,
    tuning_override: Option<SynthesisTuning>,
) -> Result<(SynthesisTuning, Vec<ReferenceSampleRef>), AppError> {
    let tuning = tuning_override.unwrap_or(profile.tuning);
    tuning.validate().map_err(AppError::Validation)?;
    Ok((tuning, profile.reference_sample_refs()))
}
