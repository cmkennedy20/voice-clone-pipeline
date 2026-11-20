//! Domain error variants for audio buffers, Gate 0 checks, and Discord waveforms.

use common::basis_points::BasisPoints;

/// Synchronous validation and Gate 0 rejection errors for audio domain values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioDomainError {
    #[error("audio sample buffer cannot be empty")]
    EmptySamples,

    #[error("audio sample buffer contains non-finite value at index {index}")]
    NonFiniteSample { index: usize },

    #[error("voiced speech duration {voiced_ms} ms is below minimum {min_ms} ms")]
    TooShortVoiced { voiced_ms: u32, min_ms: u32 },

    #[error("peak amplitude {peak_bps} is below minimum threshold {min_peak_bps}")]
    SilenceDetected {
        peak_bps: BasisPoints,
        min_peak_bps: BasisPoints,
    },

    #[error("total audio duration {total_ms} ms exceeds maximum {max_ms} ms")]
    TooLongTotal { total_ms: u32, max_ms: u32 },

    #[error("clipped sample ratio {clipped_bps} exceeds maximum {max_clipped_bps}")]
    ExcessiveClipping {
        clipped_bps: BasisPoints,
        max_clipped_bps: BasisPoints,
    },

    #[error("waveform bin count {count} must be within 1..=256")]
    InvalidWaveformBins { count: usize },

    #[error("invalid base64 waveform payload: {0}")]
    InvalidWaveformBase64(String),
}
