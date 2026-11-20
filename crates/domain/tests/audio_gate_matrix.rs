//! Layer 2 Golden Matrix Suite — Gate 0 acoustic thresholds, PCM validation, and 256-bin waveforms.

use common::basis_points::BasisPoints;
use domain::audio::{
    error::AudioDomainError,
    gate::{
        MAX_CLIPPED_SAMPLES_BPS, MAX_TOTAL_DURATION_MS, MIN_PEAK_AMPLITUDE_BPS,
        MIN_VOICED_DURATION_MS, evaluate_audio_gate,
    },
    types::{DiscordWaveform, Pcm16kMono, Pcm24kMono, RawAudioMetrics},
    waveform::quantize_256_waveform_bins,
};

struct GateCase {
    name: &'static str,
    total_ms: u32,
    voiced_ms: u32,
    clipped_bps: u32,
    peak_bps: u32,
    expected: Result<(), AudioDomainError>,
}

impl GateCase {
    const fn ok(
        name: &'static str,
        total_ms: u32,
        voiced_ms: u32,
        clipped_bps: u32,
        peak_bps: u32,
    ) -> Self {
        Self {
            name,
            total_ms,
            voiced_ms,
            clipped_bps,
            peak_bps,
            expected: Ok(()),
        }
    }

    const fn err(
        name: &'static str,
        total_ms: u32,
        voiced_ms: u32,
        clipped_bps: u32,
        peak_bps: u32,
        err: AudioDomainError,
    ) -> Self {
        Self {
            name,
            total_ms,
            voiced_ms,
            clipped_bps,
            peak_bps,
            expected: Err(err),
        }
    }
}

const GATE0_GOLDEN_CASES: [GateCase; 22] = [
    GateCase::ok(
        "01_nominal_conversational_clip_passes",
        8_500,
        7_200,
        5,
        7_500,
    ),
    GateCase::ok("02_exact_min_voiced_400ms_passes", 400, 400, 0, 3_000),
    GateCase::err(
        "03_voiced_399ms_fails_too_short",
        1_000,
        399,
        0,
        3_000,
        AudioDomainError::TooShortVoiced {
            voiced_ms: 399,
            min_ms: MIN_VOICED_DURATION_MS,
        },
    ),
    GateCase::err(
        "04_voiced_0ms_non_silent_transient_click_fails_too_short",
        1_500,
        0,
        0,
        5_000,
        AudioDomainError::TooShortVoiced {
            voiced_ms: 0,
            min_ms: MIN_VOICED_DURATION_MS,
        },
    ),
    GateCase::ok(
        "05_exact_max_total_120000ms_passes",
        120_000,
        95_000,
        20,
        8_000,
    ),
    GateCase::err(
        "06_total_120001ms_fails_too_long",
        120_001,
        95_000,
        20,
        8_000,
        AudioDomainError::TooLongTotal {
            total_ms: 120_001,
            max_ms: MAX_TOTAL_DURATION_MS,
        },
    ),
    GateCase::err(
        "07_runaway_hour_clip_fails_too_long",
        3_600_000,
        3_500_000,
        0,
        5_000,
        AudioDomainError::TooLongTotal {
            total_ms: 3_600_000,
            max_ms: MAX_TOTAL_DURATION_MS,
        },
    ),
    GateCase::ok("08_exact_min_peak_100bps_passes", 2_000, 1_200, 0, 100),
    GateCase::err(
        "09_peak_99bps_fails_silence_detected",
        2_000,
        1_200,
        0,
        99,
        AudioDomainError::SilenceDetected {
            peak_bps: BasisPoints::from_raw(99),
            min_peak_bps: MIN_PEAK_AMPLITUDE_BPS,
        },
    ),
    GateCase::err(
        "10_peak_0bps_dead_digital_silence_fails",
        3_000,
        0,
        0,
        0,
        AudioDomainError::SilenceDetected {
            peak_bps: BasisPoints::ZERO,
            min_peak_bps: MIN_PEAK_AMPLITUDE_BPS,
        },
    ),
    GateCase::ok(
        "11_exact_max_clipping_50bps_passes",
        4_000,
        3_500,
        50,
        10_000,
    ),
    GateCase::err(
        "12_clipping_51bps_fails_excessive_clipping",
        4_000,
        3_500,
        51,
        10_000,
        AudioDomainError::ExcessiveClipping {
            clipped_bps: BasisPoints::from_raw(51),
            max_clipped_bps: MAX_CLIPPED_SAMPLES_BPS,
        },
    ),
    GateCase::err(
        "13_fully_saturated_square_wave_10000bps_clipping_fails",
        1_000,
        1_000,
        10_000,
        10_000,
        AudioDomainError::ExcessiveClipping {
            clipped_bps: BasisPoints::ONE_HUNDRED_PERCENT,
            max_clipped_bps: MAX_CLIPPED_SAMPLES_BPS,
        },
    ),
    GateCase::ok(
        "14_all_four_thresholds_at_lower_edges_pass",
        400,
        400,
        0,
        100,
    ),
    GateCase::ok(
        "15_all_four_thresholds_at_upper_edges_pass",
        120_000,
        120_000,
        50,
        10_000,
    ),
    GateCase::err(
        "16_combined_too_long_and_silent_prioritizes_too_long",
        150_000,
        0,
        200,
        0,
        AudioDomainError::TooLongTotal {
            total_ms: 150_000,
            max_ms: MAX_TOTAL_DURATION_MS,
        },
    ),
    GateCase::err(
        "17_combined_silent_and_short_voiced_prioritizes_silence",
        500,
        100,
        90,
        25,
        AudioDomainError::SilenceDetected {
            peak_bps: BasisPoints::from_raw(25),
            min_peak_bps: MIN_PEAK_AMPLITUDE_BPS,
        },
    ),
    GateCase::err(
        "18_combined_short_voiced_and_clipped_prioritizes_short_voiced",
        2_000,
        250,
        300,
        10_000,
        AudioDomainError::TooShortVoiced {
            voiced_ms: 250,
            min_ms: MIN_VOICED_DURATION_MS,
        },
    ),
    GateCase::ok(
        "19_long_pause_clip_120s_total_with_400ms_voiced_passes",
        120_000,
        400,
        49,
        101,
    ),
    GateCase::err(
        "20_long_pause_clip_120s_total_with_399ms_voiced_fails",
        120_000,
        399,
        49,
        101,
        AudioDomainError::TooShortVoiced {
            voiced_ms: 399,
            min_ms: MIN_VOICED_DURATION_MS,
        },
    ),
    GateCase::ok(
        "21_whisper_quiet_speech_100bps_peak_401ms_voiced_passes",
        600,
        401,
        0,
        100,
    ),
    GateCase::err(
        "22_zero_length_metrics_fails_silence_before_voiced_check",
        0,
        0,
        0,
        0,
        AudioDomainError::SilenceDetected {
            peak_bps: BasisPoints::ZERO,
            min_peak_bps: MIN_PEAK_AMPLITUDE_BPS,
        },
    ),
];

#[test]
fn gate0_boundary_and_precedence_golden_matrix() {
    assert!(GATE0_GOLDEN_CASES.len() >= 20);
    for case in &GATE0_GOLDEN_CASES {
        let metrics = RawAudioMetrics {
            total_duration_ms: case.total_ms,
            voiced_duration_ms: case.voiced_ms,
            clipped_samples_bps: BasisPoints::from_raw(case.clipped_bps),
            peak_amplitude_bps: BasisPoints::from_raw(case.peak_bps),
        };
        let actual = evaluate_audio_gate(&metrics);
        assert_eq!(actual, case.expected, "failed gate case `{}`", case.name);
    }
}

#[test]
fn pcm16k_and_pcm24k_sample_validation_and_duration_matrix() {
    assert_eq!(Pcm16kMono::new(vec![]), Err(AudioDomainError::EmptySamples));
    assert_eq!(Pcm24kMono::new(vec![]), Err(AudioDomainError::EmptySamples));

    assert_eq!(
        Pcm16kMono::new(vec![0.0, -0.5, f32::NAN]),
        Err(AudioDomainError::NonFiniteSample { index: 2 })
    );
    assert_eq!(
        Pcm16kMono::new(vec![f32::NEG_INFINITY, 0.0]),
        Err(AudioDomainError::NonFiniteSample { index: 0 })
    );
    assert_eq!(
        Pcm24kMono::new(vec![0.25, f32::INFINITY]),
        Err(AudioDomainError::NonFiniteSample { index: 1 })
    );

    // 6,384 samples @ 16 kHz = 399 ms; 6,400 samples @ 16 kHz = 400 ms
    assert_eq!(
        Pcm16kMono::new(vec![0.1; 6_384]).unwrap().duration_ms(),
        399
    );
    assert_eq!(
        Pcm16kMono::new(vec![0.1; 6_400]).unwrap().duration_ms(),
        400
    );

    // 9,576 samples @ 24 kHz = 399 ms; 9,600 samples @ 24 kHz = 400 ms; 240,000 samples = 10,000 ms
    assert_eq!(
        Pcm24kMono::new(vec![0.1; 9_576]).unwrap().duration_ms(),
        399
    );
    assert_eq!(
        Pcm24kMono::new(vec![0.1; 9_600]).unwrap().duration_ms(),
        400
    );
    assert_eq!(
        Pcm24kMono::new(vec![0.1; 240_000]).unwrap().duration_ms(),
        10_000
    );
}

#[test]
fn waveform_256_bin_base64_length_and_decoding_matrix() {
    // 1 byte -> 4 chars RFC 4648 base64; 256 bytes -> ceil(256 / 3) * 4 = 344 chars RFC 4648 base64
    let wf_one = DiscordWaveform::from_bins(&[128_u8]).unwrap();
    assert_eq!(wf_one.bin_count(), 1);
    assert_eq!(wf_one.as_str().len(), 4);
    assert_eq!(wf_one.decode_bins().unwrap(), vec![128_u8]);

    let ramp: Vec<u8> = (0_u8..=255_u8).collect();
    let wf_full = DiscordWaveform::from_bins(&ramp).unwrap();
    assert_eq!(wf_full.bin_count(), 256);
    assert_eq!(wf_full.as_str().len(), 344);
    assert_eq!(wf_full.decode_bins().unwrap(), ramp);

    let reparsed = DiscordWaveform::parse_base64(wf_full.as_str()).unwrap();
    assert_eq!(reparsed, wf_full);

    assert!(matches!(
        DiscordWaveform::parse_base64("not!valid!base64"),
        Err(AudioDomainError::InvalidWaveformBase64(_))
    ));
    assert_eq!(
        DiscordWaveform::parse_base64(""),
        Err(AudioDomainError::InvalidWaveformBins { count: 0 })
    );

    assert_eq!(quantize_256_waveform_bins(&[]).bin_count(), 1);
    assert_eq!(quantize_256_waveform_bins(&vec![0.4; 255]).bin_count(), 255);
    assert_eq!(quantize_256_waveform_bins(&vec![0.4; 256]).bin_count(), 256);

    let wf_speech_len = quantize_256_waveform_bins(&vec![1.0; 16_000]);
    assert_eq!(wf_speech_len.bin_count(), 256);
    assert_eq!(wf_speech_len.as_str().len(), 344);
    assert_eq!(wf_speech_len.decode_bins().unwrap(), vec![255_u8; 256]);
}
