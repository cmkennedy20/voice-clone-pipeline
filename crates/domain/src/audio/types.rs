//! Validated PCM buffers, raw audio metrics, and Discord `flags: 8192` waveform types.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use common::basis_points::BasisPoints;

use crate::audio::error::AudioDomainError;

fn validate_pcm_samples(samples: &[f32]) -> Result<(), AudioDomainError> {
    if samples.is_empty() {
        return Err(AudioDomainError::EmptySamples);
    }
    for (index, sample) in samples.iter().enumerate() {
        if !sample.is_finite() {
            return Err(AudioDomainError::NonFiniteSample { index });
        }
    }
    Ok(())
}

fn sample_count_to_duration_ms(sample_count: usize, sample_rate_hz: u32) -> u32 {
    let count = u128::try_from(sample_count).unwrap_or(u128::MAX);
    let ms = (count * 1_000) / u128::from(sample_rate_hz);
    u32::try_from(ms).unwrap_or(u32::MAX)
}

/// Validated non-empty, finite `16 kHz` mono `f32` PCM buffer for ASR input.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm16kMono {
    samples: Vec<f32>,
}

impl Pcm16kMono {
    /// Fixed sample rate in Hertz (`16_000 Hz`).
    pub const SAMPLE_RATE_HZ: u32 = 16_000;

    /// Validates that `samples` is non-empty and contains strictly finite `f32` values.
    pub fn new(samples: Vec<f32>) -> Result<Self, AudioDomainError> {
        validate_pcm_samples(&samples)?;
        Ok(Self { samples })
    }

    /// Borrows the validated `16 kHz` mono sample slice.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Consumes the wrapper and returns the inner sample vector.
    #[must_use]
    pub fn into_samples(self) -> Vec<f32> {
        self.samples
    }

    /// Computes clip duration in milliseconds (`samples.len() * 1000 / 16_000`).
    #[must_use]
    pub fn duration_ms(&self) -> u32 {
        sample_count_to_duration_ms(self.samples.len(), Self::SAMPLE_RATE_HZ)
    }
}

/// Validated non-empty, finite `24 kHz` mono `f32` PCM buffer produced by TTS synthesis.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm24kMono {
    samples: Vec<f32>,
}

impl Pcm24kMono {
    /// Fixed sample rate in Hertz (`24_000 Hz`).
    pub const SAMPLE_RATE_HZ: u32 = 24_000;

    /// Validates that `samples` is non-empty and contains strictly finite `f32` values.
    pub fn new(samples: Vec<f32>) -> Result<Self, AudioDomainError> {
        validate_pcm_samples(&samples)?;
        Ok(Self { samples })
    }

    /// Borrows the validated `24 kHz` mono sample slice.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Consumes the wrapper and returns the inner sample vector.
    #[must_use]
    pub fn into_samples(self) -> Vec<f32> {
        self.samples
    }

    /// Computes clip duration in milliseconds (`samples.len() * 1000 / 24_000`).
    #[must_use]
    pub fn duration_ms(&self) -> u32 {
        sample_count_to_duration_ms(self.samples.len(), Self::SAMPLE_RATE_HZ)
    }
}

/// Pre-GPU Gate 0 acoustic measurements computed during audio demux and VAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RawAudioMetrics {
    pub total_duration_ms: u32,
    pub voiced_duration_ms: u32,
    pub clipped_samples_bps: BasisPoints,
    pub peak_amplitude_bps: BasisPoints,
}

/// Standard `RFC 4648` base64-encoded `1..=256` byte waveform preview for Discord voice bubbles.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscordWaveform {
    base64_bins: String,
    bin_count: usize,
}

impl DiscordWaveform {
    /// Minimum allowed waveform bin count.
    pub const MIN_BINS: usize = 1;
    /// Maximum allowed waveform bin count (`256` bytes).
    pub const MAX_BINS: usize = 256;

    /// Encodes `1..=256` quantized `u8` amplitude bins using standard `RFC 4648` base64.
    pub fn from_bins(bins: &[u8]) -> Result<Self, AudioDomainError> {
        if !(Self::MIN_BINS..=Self::MAX_BINS).contains(&bins.len()) {
            return Err(AudioDomainError::InvalidWaveformBins { count: bins.len() });
        }
        Ok(Self {
            base64_bins: STANDARD.encode(bins),
            bin_count: bins.len(),
        })
    }

    /// Parses and validates an existing standard base64 waveform string (`1..=256` decoded bytes).
    pub fn parse_base64(encoded: &str) -> Result<Self, AudioDomainError> {
        let decoded = STANDARD
            .decode(encoded)
            .map_err(|err| AudioDomainError::InvalidWaveformBase64(err.to_string()))?;
        Self::from_bins(&decoded)
    }

    /// Returns the `RFC 4648` base64 waveform string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.base64_bins
    }

    /// Returns the number of quantized bins (`1..=256`).
    #[must_use]
    pub const fn bin_count(&self) -> usize {
        self.bin_count
    }

    /// Decodes the base64 payload back into raw `u8` amplitude bins.
    pub fn decode_bins(&self) -> Result<Vec<u8>, AudioDomainError> {
        let decoded = STANDARD
            .decode(&self.base64_bins)
            .map_err(|err| AudioDomainError::InvalidWaveformBase64(err.to_string()))?;
        if !(Self::MIN_BINS..=Self::MAX_BINS).contains(&decoded.len()) {
            return Err(AudioDomainError::InvalidWaveformBins {
                count: decoded.len(),
            });
        }
        Ok(decoded)
    }
}

impl Default for DiscordWaveform {
    fn default() -> Self {
        Self {
            base64_bins: STANDARD.encode([0_u8]),
            bin_count: 1,
        }
    }
}

/// Dual-encoded `48 kHz` Ogg/Opus and `48 kHz` WAV output bundle ready for delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedAudioBundle {
    pub ogg_opus_bytes: Vec<u8>,
    pub wav_48k_bytes: Vec<u8>,
    pub waveform: DiscordWaveform,
    pub duration_ms: u32,
}

#[cfg(test)]
mod tests {
    use crate::audio::{
        error::AudioDomainError,
        types::{DiscordWaveform, Pcm16kMono, Pcm24kMono},
    };

    #[test]
    fn pcm_buffers_reject_empty_and_non_finite_samples() {
        assert_eq!(
            Pcm16kMono::new(Vec::new()),
            Err(AudioDomainError::EmptySamples)
        );
        assert_eq!(
            Pcm16kMono::new(vec![0.1, f32::NAN, 0.2]),
            Err(AudioDomainError::NonFiniteSample { index: 1 })
        );
        assert_eq!(
            Pcm24kMono::new(vec![0.0, 0.5, f32::INFINITY]),
            Err(AudioDomainError::NonFiniteSample { index: 2 })
        );
    }

    #[test]
    fn pcm_duration_matches_sample_rate() {
        let pcm16 = Pcm16kMono::new(vec![0.25; 24_000]).unwrap();
        assert_eq!(pcm16.duration_ms(), 1_500);
        assert_eq!(pcm16.samples().len(), 24_000);
        assert_eq!(pcm16.into_samples().len(), 24_000);

        let pcm24 = Pcm24kMono::new(vec![-0.25; 36_000]).unwrap();
        assert_eq!(pcm24.duration_ms(), 1_500);
        assert_eq!(pcm24.samples().len(), 36_000);
        assert_eq!(pcm24.into_samples().len(), 36_000);
    }

    #[test]
    fn discord_waveform_enforces_bin_bounds_and_round_trips() {
        assert_eq!(
            DiscordWaveform::from_bins(&[]),
            Err(AudioDomainError::InvalidWaveformBins { count: 0 })
        );
        assert_eq!(
            DiscordWaveform::from_bins(&[128_u8; 257]),
            Err(AudioDomainError::InvalidWaveformBins { count: 257 })
        );

        let bins = [0_u8, 64, 128, 255];
        let wf = DiscordWaveform::from_bins(&bins).unwrap();
        assert_eq!(wf.bin_count(), 4);
        assert_eq!(wf.as_str(), "AECA/w==");
        assert_eq!(wf.decode_bins().unwrap(), bins);
        assert_eq!(DiscordWaveform::parse_base64("AECA/w==").unwrap(), wf);
    }
}
