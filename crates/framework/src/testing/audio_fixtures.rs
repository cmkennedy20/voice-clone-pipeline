//! Deterministic synthetic `PCM` and `RIFF WAV` audio fixtures for automated `Gate 0` and codec tests.

use std::io::Cursor;

use domain::audio::types::{Pcm16kMono, Pcm24kMono};

const RATE_16K: u16 = 16_000;
const RATE_24K: u16 = 24_000;

fn harmonic_sin(sample_idx: usize, freq_hz: u16, sample_rate_hz: u16) -> f32 {
    let rate_usize = usize::from(sample_rate_hz.max(1));
    let phase_index =
        u16::try_from(sample_idx.saturating_mul(usize::from(freq_hz)) % rate_usize).unwrap_or(0);
    let angle = std::f32::consts::TAU * f32::from(phase_index) / f32::from(sample_rate_hz.max(1));
    angle.sin()
}

fn generate_speech_like_samples(duration_ms: u32, sample_rate_hz: u16) -> Vec<f32> {
    let total_u64 = ((u64::from(duration_ms) * u64::from(sample_rate_hz)) / 1_000).max(1);
    let sample_count = usize::try_from(total_u64).unwrap_or(1).max(1);
    let mut out = Vec::with_capacity(sample_count);

    for idx in 0..sample_count {
        let f0 = harmonic_sin(idx, 180, sample_rate_hz);
        let f1 = harmonic_sin(idx, 540, sample_rate_hz);
        let f2 = harmonic_sin(idx, 900, sample_rate_hz);
        let envelope = 0.65_f32 + 0.25_f32 * harmonic_sin(idx, 4, sample_rate_hz);
        let voiced_wave = 0.50_f32 * f0 + 0.30_f32 * f1 + 0.15_f32 * f2;
        let sample = (0.62_f32 * envelope * voiced_wave).clamp(-0.90_f32, 0.90_f32);
        out.push(if sample.is_finite() { sample } else { 0.0_f32 });
    }
    out
}

fn unit_f32_to_i16(sample: f32) -> i16 {
    if !sample.is_finite() || sample == 0.0 {
        return 0;
    }
    let clamped = f64::from(sample).clamp(-1.0, 1.0);
    let scaled = clamped * f64::from(i16::MAX);
    let mut low = i32::from(-i16::MAX);
    let mut high = i32::from(i16::MAX);
    while low < high {
        let mid = low + (high - low + 1) / 2;
        if f64::from(mid) - 0.5 <= scaled {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    i16::try_from(low).unwrap_or(0)
}

fn encode_pcm16_wav(samples: &[f32], sample_rate_hz: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sample_rate_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    if let Ok(mut writer) = hound::WavWriter::new(&mut cursor, spec) {
        for &sample in samples {
            let _ = writer.write_sample(unit_f32_to_i16(sample));
        }
        let _ = writer.finalize();
    }
    cursor.into_inner()
}

/// Builds a deterministic speech-like `16 kHz` mono `PCM` buffer of `duration_ms` milliseconds.
#[must_use]
pub fn synthetic_16k_pcm(duration_ms: u32) -> Pcm16kMono {
    let samples = generate_speech_like_samples(duration_ms, RATE_16K);
    if let Ok(pcm) = Pcm16kMono::new(samples) {
        return pcm;
    }
    loop {
        if let Ok(fallback) = Pcm16kMono::new(vec![0.0_f32]) {
            return fallback;
        }
    }
}

/// Builds a deterministic speech-like `24 kHz` mono `PCM` buffer of `duration_ms` milliseconds.
#[must_use]
pub fn synthetic_24k_pcm(duration_ms: u32) -> Pcm24kMono {
    let samples = generate_speech_like_samples(duration_ms, RATE_24K);
    if let Ok(pcm) = Pcm24kMono::new(samples) {
        return pcm;
    }
    loop {
        if let Ok(fallback) = Pcm24kMono::new(vec![0.0_f32]) {
            return fallback;
        }
    }
}

/// Builds a `24 kHz` `16-bit` mono `RIFF WAV` of `duration_ms` with voiced speech-like harmonics.
#[must_use]
pub fn synthetic_speech_wav_ms(duration_ms: u32) -> Vec<u8> {
    let samples = generate_speech_like_samples(duration_ms, RATE_24K);
    encode_pcm16_wav(&samples, u32::from(RATE_24K))
}

/// Builds a nominal `1_600 ms` voiced `24 kHz` `RIFF WAV` that cleanly passes `Gate 0`.
#[must_use]
pub fn synthetic_speech_wav() -> Vec<u8> {
    synthetic_speech_wav_ms(1_600)
}

/// Builds a `250 ms` dead-silent `24 kHz` `RIFF WAV` that fails `Gate 0` (`SilenceDetected`).
#[must_use]
pub fn synthetic_silent_short_wav() -> Vec<u8> {
    let silent_samples = vec![0.0_f32; 6_000];
    encode_pcm16_wav(&silent_samples, u32::from(RATE_24K))
}

/// Builds a `1_500 ms` heavily clipped (`> 150 bps`) `24 kHz` `RIFF WAV` that fails `Gate 0`.
#[must_use]
pub fn synthetic_clipped_wav() -> Vec<u8> {
    let base = generate_speech_like_samples(1_500, RATE_24K);
    let overdriven: Vec<f32> = base
        .into_iter()
        .map(|s| (s * 4.0_f32).clamp(-1.0_f32, 1.0_f32))
        .collect();
    encode_pcm16_wav(&overdriven, u32::from(RATE_24K))
}
