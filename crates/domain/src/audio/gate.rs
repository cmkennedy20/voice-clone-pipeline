//! Pre-GPU Gate 0 audio hygiene guard rejecting silence, clipping, and runaway clips.

use common::basis_points::BasisPoints;

use crate::audio::{error::AudioDomainError, types::RawAudioMetrics};

/// Minimum voiced speech duration required to enter GPU inference (`400 ms`).
pub const MIN_VOICED_DURATION_MS: u32 = 400;

/// Maximum total clip duration allowed (`120_000 ms` = `2 minutes`).
pub const MAX_TOTAL_DURATION_MS: u32 = 120_000;

/// Maximum allowed clipped sample ratio (`50 bps` = `0.50%`).
pub const MAX_CLIPPED_SAMPLES_BPS: BasisPoints = BasisPoints::from_raw(50);

/// Minimum peak amplitude required to distinguish speech from dead air (`100 bps` = `1.00%`).
pub const MIN_PEAK_AMPLITUDE_BPS: BasisPoints = BasisPoints::from_raw(100);

/// Evaluates Gate 0 acoustic thresholds before acquiring any GPU inference resources.
pub fn evaluate_audio_gate(metrics: &RawAudioMetrics) -> Result<(), AudioDomainError> {
    if metrics.total_duration_ms > MAX_TOTAL_DURATION_MS {
        return Err(AudioDomainError::TooLongTotal {
            total_ms: metrics.total_duration_ms,
            max_ms: MAX_TOTAL_DURATION_MS,
        });
    }
    if metrics.peak_amplitude_bps < MIN_PEAK_AMPLITUDE_BPS {
        return Err(AudioDomainError::SilenceDetected {
            peak_bps: metrics.peak_amplitude_bps,
            min_peak_bps: MIN_PEAK_AMPLITUDE_BPS,
        });
    }
    if metrics.voiced_duration_ms < MIN_VOICED_DURATION_MS {
        return Err(AudioDomainError::TooShortVoiced {
            voiced_ms: metrics.voiced_duration_ms,
            min_ms: MIN_VOICED_DURATION_MS,
        });
    }
    if metrics.clipped_samples_bps > MAX_CLIPPED_SAMPLES_BPS {
        return Err(AudioDomainError::ExcessiveClipping {
            clipped_bps: metrics.clipped_samples_bps,
            max_clipped_bps: MAX_CLIPPED_SAMPLES_BPS,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use common::basis_points::BasisPoints;

    use super::{
        MAX_CLIPPED_SAMPLES_BPS, MAX_TOTAL_DURATION_MS, MIN_PEAK_AMPLITUDE_BPS,
        MIN_VOICED_DURATION_MS, evaluate_audio_gate,
    };
    use crate::audio::{error::AudioDomainError, types::RawAudioMetrics};

    fn nominal_metrics() -> RawAudioMetrics {
        RawAudioMetrics {
            total_duration_ms: 5_000,
            voiced_duration_ms: 4_200,
            clipped_samples_bps: BasisPoints::from_raw(10),
            peak_amplitude_bps: BasisPoints::from_raw(6_500),
        }
    }

    #[test]
    fn accepts_exact_inclusive_threshold_boundaries() {
        let lower_edge = RawAudioMetrics {
            total_duration_ms: MIN_VOICED_DURATION_MS,
            voiced_duration_ms: MIN_VOICED_DURATION_MS,
            clipped_samples_bps: BasisPoints::ZERO,
            peak_amplitude_bps: MIN_PEAK_AMPLITUDE_BPS,
        };
        assert_eq!(evaluate_audio_gate(&lower_edge), Ok(()));

        let upper_edge = RawAudioMetrics {
            total_duration_ms: MAX_TOTAL_DURATION_MS,
            voiced_duration_ms: MAX_TOTAL_DURATION_MS,
            clipped_samples_bps: MAX_CLIPPED_SAMPLES_BPS,
            peak_amplitude_bps: BasisPoints::ONE_HUNDRED_PERCENT,
        };
        assert_eq!(evaluate_audio_gate(&upper_edge), Ok(()));
    }

    #[test]
    fn rejects_runaway_duration_above_120_seconds() {
        let mut metrics = nominal_metrics();
        metrics.total_duration_ms = 120_001;
        assert_eq!(
            evaluate_audio_gate(&metrics),
            Err(AudioDomainError::TooLongTotal {
                total_ms: 120_001,
                max_ms: 120_000,
            })
        );
    }

    #[test]
    fn rejects_silent_clip_below_100_bps_peak() {
        let mut metrics = nominal_metrics();
        metrics.peak_amplitude_bps = BasisPoints::from_raw(99);
        assert_eq!(
            evaluate_audio_gate(&metrics),
            Err(AudioDomainError::SilenceDetected {
                peak_bps: BasisPoints::from_raw(99),
                min_peak_bps: MIN_PEAK_AMPLITUDE_BPS,
            })
        );
    }

    #[test]
    fn rejects_short_voiced_clip_below_400_ms() {
        let mut metrics = nominal_metrics();
        metrics.voiced_duration_ms = 399;
        assert_eq!(
            evaluate_audio_gate(&metrics),
            Err(AudioDomainError::TooShortVoiced {
                voiced_ms: 399,
                min_ms: MIN_VOICED_DURATION_MS,
            })
        );
    }

    #[test]
    fn rejects_clipped_clip_above_50_bps() {
        let mut metrics = nominal_metrics();
        metrics.clipped_samples_bps = BasisPoints::from_raw(51);
        assert_eq!(
            evaluate_audio_gate(&metrics),
            Err(AudioDomainError::ExcessiveClipping {
                clipped_bps: BasisPoints::from_raw(51),
                max_clipped_bps: MAX_CLIPPED_SAMPLES_BPS,
            })
        );
    }
}
