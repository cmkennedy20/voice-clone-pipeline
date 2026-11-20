//! Pure panic-free `256`-bin waveform quantizer for Discord `IS_VOICE_MESSAGE` previews.

use crate::audio::types::DiscordWaveform;

fn sanitize_unit_magnitude(sample: f32) -> f64 {
    if sample.is_finite() {
        f64::from(sample).abs().min(1.0)
    } else {
        0.0
    }
}

fn quantize_unit_to_u8(unit: f64) -> u8 {
    if !unit.is_finite() || unit <= 0.0 {
        return 0;
    }
    if unit >= 1.0 {
        return u8::MAX;
    }
    let scaled = unit * 255.0;
    let mut low: u8 = 0;
    let mut high: u8 = u8::MAX;
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if f64::from(mid) - 0.5 <= scaled {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    low
}

fn chunk_rms_bin(chunk: &[f32]) -> u8 {
    if chunk.is_empty() {
        return 0;
    }
    let mut sum_sq = 0.0_f64;
    for &sample in chunk {
        let mag = sanitize_unit_magnitude(sample);
        sum_sq += mag * mag;
    }
    let len_f64 = u32::try_from(chunk.len()).map_or(f64::MAX, f64::from);
    let rms = (sum_sq / len_f64).sqrt().clamp(0.0, 1.0);
    quantize_unit_to_u8(rms)
}

/// Quantizes arbitrary PCM samples into a `1..=256` bin [`DiscordWaveform`].
///
/// - Empty input produces a single zero bin (`[0]`).
/// - Inputs of length `1..256` produce `samples.len()` bins (`1` sample per bin).
/// - Inputs of length `>= 256` produce `256` evenly partitioned RMS bins.
/// - Non-finite (`NaN`, `±Inf`) samples are sanitized to `0.0`.
#[must_use]
pub fn quantize_256_waveform_bins(samples: &[f32]) -> DiscordWaveform {
    if samples.is_empty() {
        return DiscordWaveform::default();
    }

    let total_len = samples.len();
    let max_bins = DiscordWaveform::MAX_BINS;

    let bins: Vec<u8> = if total_len < max_bins {
        samples
            .iter()
            .map(|&sample| quantize_unit_to_u8(sanitize_unit_magnitude(sample)))
            .collect()
    } else {
        let q = total_len / max_bins;
        let r = total_len % max_bins;
        (0..max_bins)
            .map(|bin_idx| {
                let start = bin_idx * q + (bin_idx * r) / max_bins;
                let end = (bin_idx + 1) * q + ((bin_idx + 1) * r) / max_bins;
                chunk_rms_bin(&samples[start..end])
            })
            .collect()
    };

    DiscordWaveform::from_bins(&bins).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::quantize_256_waveform_bins;

    #[test]
    fn empty_input_returns_single_zero_bin() {
        let wf = quantize_256_waveform_bins(&[]);
        assert_eq!(wf.bin_count(), 1);
        assert_eq!(wf.decode_bins().unwrap(), vec![0_u8]);
    }

    #[test]
    fn non_finite_values_are_sanitized_to_zero() {
        let wf = quantize_256_waveform_bins(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0]);
        assert_eq!(wf.decode_bins().unwrap(), vec![0_u8, 0, 0, 255]);
    }

    #[test]
    fn constant_half_and_full_scale_quantize_predictably() {
        let zeros = quantize_256_waveform_bins(&vec![0.0_f32; 1_024]);
        assert_eq!(zeros.bin_count(), 256);
        assert_eq!(zeros.decode_bins().unwrap(), vec![0_u8; 256]);

        let full = quantize_256_waveform_bins(&vec![-1.0_f32; 1_024]);
        assert_eq!(full.bin_count(), 256);
        assert_eq!(full.decode_bins().unwrap(), vec![255_u8; 256]);

        let half = quantize_256_waveform_bins(&vec![0.5_f32; 512]);
        assert_eq!(half.decode_bins().unwrap(), vec![128_u8; 256]);
    }

    proptest! {
        #[test]
        fn never_panics_and_always_emits_valid_1_to_256_bins_on_any_f32_vec(
            samples in proptest::collection::vec(proptest::num::f32::ANY, 0..=5_000)
        ) {
            let wf = quantize_256_waveform_bins(&samples);
            let bins = wf.decode_bins().unwrap();
            prop_assert!((1..=256).contains(&bins.len()));
            prop_assert_eq!(wf.bin_count(), bins.len());
            if samples.is_empty() {
                prop_assert_eq!(bins, vec![0_u8]);
            } else if samples.len() < 256 {
                prop_assert_eq!(bins.len(), samples.len());
            } else {
                prop_assert_eq!(bins.len(), 256);
            }
        }

        #[test]
        fn all_zero_samples_always_produce_all_zero_bins(len in 1_usize..=2_048) {
            let samples = vec![0.0_f32; len];
            let wf = quantize_256_waveform_bins(&samples);
            let bins = wf.decode_bins().unwrap();
            prop_assert!(bins.iter().all(|&b| b == 0));
        }

        #[test]
        fn full_scale_constant_amplitude_always_produces_max_bins(
            len in 1_usize..=2_048,
            negative in proptest::bool::ANY,
        ) {
            let amp = if negative { -1.0_f32 } else { 1.0_f32 };
            let samples = vec![amp; len];
            let wf = quantize_256_waveform_bins(&samples);
            let bins = wf.decode_bins().unwrap();
            prop_assert!(bins.iter().all(|&b| b == u8::MAX));
        }
    }
}
