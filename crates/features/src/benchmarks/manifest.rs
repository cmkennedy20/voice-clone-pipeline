//! Golden benchmark manifest parser and `M1` (`Gate 0`) pre-`GPU` trap evaluator.

use common::basis_points::BasisPoints;
use domain::audio::{error::AudioDomainError, gate::evaluate_audio_gate, types::RawAudioMetrics};

use crate::error::AppError;

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct GoldenManifest {
    pub manifest_version: String,
    pub thresholds: ManifestThresholds,
    pub gate0_traps: Vec<Gate0TrapSpec>,
    pub utterances: Vec<GoldenUtteranceSpec>,
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub(crate) struct ManifestThresholds {
    pub gate0_accuracy_bps: u32,
    pub digit_hygiene_bps: u32,
    pub max_mean_cer_bps: u32,
    pub max_clip_cer_bps: u32,
    pub min_speaker_sim_bps: u32,
    pub format_compliance_bps: u32,
    pub max_p95_latency_ms: u32,
    pub max_peak_vram_mib: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Gate0TrapSpec {
    pub id: String,
    pub total_duration_ms: u32,
    pub voiced_duration_ms: u32,
    pub clipped_samples_bps: u32,
    pub expected_rejection_category: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct GoldenUtteranceSpec {
    pub id: String,
    pub category: String,
    pub english_prompt: String,
    #[serde(default)]
    pub forbidden_substrings: Vec<String>,
}

pub(crate) fn parse_golden_manifest(manifest_json: &str) -> Result<GoldenManifest, AppError> {
    let manifest: GoldenManifest = serde_json::from_str(manifest_json)
        .map_err(|err| AppError::Validation(format!("invalid golden manifest JSON: {err}")))?;
    if manifest.gate0_traps.is_empty() {
        return Err(AppError::Validation(
            "golden manifest must include at least one gate0_traps entry".to_owned(),
        ));
    }
    if manifest.utterances.is_empty() {
        return Err(AppError::Validation(
            "golden manifest must include at least one utterances entry".to_owned(),
        ));
    }
    if u16::try_from(manifest.utterances.len()).is_err() {
        return Err(AppError::Validation(
            "golden manifest utterance count exceeds u16::MAX".to_owned(),
        ));
    }
    Ok(manifest)
}

fn rejection_category_slug(err: &AudioDomainError) -> &'static str {
    match err {
        AudioDomainError::SilenceDetected { .. } | AudioDomainError::TooShortVoiced { .. } => {
            "silent_too_short"
        }
        AudioDomainError::ExcessiveClipping { .. } => "heavily_clipped",
        AudioDomainError::TooLongTotal { .. } => "runaway_duration",
        AudioDomainError::EmptySamples
        | AudioDomainError::NonFiniteSample { .. }
        | AudioDomainError::InvalidWaveformBins { .. }
        | AudioDomainError::InvalidWaveformBase64(_) => "other",
    }
}

pub(crate) fn evaluate_gate0_traps(traps: &[Gate0TrapSpec]) -> BasisPoints {
    let mut rejected_correctly: u64 = 0;
    for trap in traps {
        let peak_amplitude_bps = if trap.clipped_samples_bps > 0 {
            BasisPoints::ONE_HUNDRED_PERCENT
        } else if trap.voiced_duration_ms == 0 {
            BasisPoints::ZERO
        } else {
            BasisPoints::from_raw(5_000)
        };
        let metrics = RawAudioMetrics {
            total_duration_ms: trap.total_duration_ms,
            voiced_duration_ms: trap.voiced_duration_ms,
            clipped_samples_bps: BasisPoints::from_raw(trap.clipped_samples_bps),
            peak_amplitude_bps,
        };
        match evaluate_audio_gate(&metrics) {
            Err(err) => {
                let actual = rejection_category_slug(&err);
                if trap.expected_rejection_category.is_empty()
                    || actual == trap.expected_rejection_category
                {
                    rejected_correctly = rejected_correctly.saturating_add(1);
                } else {
                    tracing::warn!(
                        trap_id = %trap.id,
                        expected = %trap.expected_rejection_category,
                        actual,
                        "Gate 0 trap rejected under unexpected category"
                    );
                }
            }
            Ok(()) => {
                tracing::warn!(trap_id = %trap.id, "Gate 0 trap leaked past pre-GPU gate");
            }
        }
    }
    let total = u64::try_from(traps.len()).unwrap_or(0);
    BasisPoints::from_ratio_round_half_up(rejected_correctly, total)
}

#[cfg(test)]
mod tests {
    use common::basis_points::BasisPoints;

    use super::{evaluate_gate0_traps, parse_golden_manifest};
    use crate::error::AppError;

    #[test]
    fn parses_golden_manifest_and_scores_100_percent_on_all_three_gate0_traps() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../evals/golden_manifest.json"
        ));
        let manifest = parse_golden_manifest(raw).unwrap();
        assert_eq!(manifest.manifest_version, "1.0.0");
        assert_eq!(manifest.gate0_traps.len(), 3);
        assert_eq!(manifest.utterances.len(), 15);

        let m1_bps = evaluate_gate0_traps(&manifest.gate0_traps);
        assert_eq!(m1_bps, BasisPoints::ONE_HUNDRED_PERCENT);
    }

    #[test]
    fn rejects_malformed_or_empty_manifests() {
        assert!(matches!(
            parse_golden_manifest("{not-json}"),
            Err(AppError::Validation(_))
        ));
    }
}
