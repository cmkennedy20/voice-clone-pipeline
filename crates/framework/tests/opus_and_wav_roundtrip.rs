//! Layer 3 real `Ogg/Opus` (`RFC 7845`) + `48 kHz` `RIFF WAV` round-trip and `Symphonia` demux suite.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Cursor, ErrorKind};

use common::basis_points::BasisPoints;
use domain::{
    audio::{error::AudioDomainError, gate::evaluate_audio_gate},
    ports::AudioCodecEngine,
};
use framework::{
    dsp::opus_and_wav::{OPUS_SAMPLE_RATE_HZ, RealAudioCodecEngine},
    testing::audio_fixtures::{
        synthetic_24k_pcm, synthetic_clipped_wav, synthetic_silent_short_wav, synthetic_speech_wav,
        synthetic_speech_wav_ms,
    },
};
use symphonia::core::{
    audio::Channels,
    codecs::CODEC_TYPE_OPUS,
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

fn compute_rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|&s| {
            let v = f64::from(s);
            v * v
        })
        .sum();
    let len_f64 = u32::try_from(samples.len()).map_or(1.0_f64, f64::from);
    (sum_sq / len_f64).sqrt()
}

#[test]
fn encode_bundle_emits_ogg_opus_and_riff_wav_magic_headers_with_256_bin_waveform() {
    let codec = RealAudioCodecEngine::new();
    let pcm_24k = synthetic_24k_pcm(1_500);
    let bundle = codec.encode_bundle(&pcm_24k).unwrap();

    assert!(bundle.ogg_opus_bytes.len() > 64);
    assert_eq!(&bundle.ogg_opus_bytes[0..4], b"OggS");
    assert!(
        bundle
            .ogg_opus_bytes
            .windows(b"OpusHead".len())
            .any(|w| w == b"OpusHead"),
        "RFC 7845 stream must include OpusHead identification header"
    );
    assert!(
        bundle
            .ogg_opus_bytes
            .windows(b"OpusTags".len())
            .any(|w| w == b"OpusTags"),
        "RFC 7845 stream must include OpusTags comment header"
    );
    assert!(
        bundle
            .ogg_opus_bytes
            .windows(b"voice-clone-pipeline".len())
            .any(|w| w == b"voice-clone-pipeline"),
        "RFC 7845 OpusTags must embed voice-clone-pipeline vendor string"
    );

    assert!(bundle.wav_48k_bytes.len() > 44);
    assert_eq!(&bundle.wav_48k_bytes[0..4], b"RIFF");
    assert_eq!(&bundle.wav_48k_bytes[8..12], b"WAVE");

    assert_eq!(bundle.duration_ms, 1_500);
    assert_eq!(bundle.waveform.bin_count(), 256);
    let bins = bundle.waveform.decode_bins().unwrap();
    assert_eq!(bins.len(), 256);
    assert!(
        bins.iter().filter(|&&b| b >= 20).count() >= 200,
        "voiced waveform bins must reflect non-zero speech envelope"
    );
}

#[test]
fn symphonia_demuxes_encoded_48khz_mono_ogg_opus_with_sub_40ms_drift_and_signal_energy() {
    let codec = RealAudioCodecEngine::new();
    let source = synthetic_24k_pcm(1_500);
    let bundle = codec.encode_bundle(&source).unwrap();

    let mss = MediaSourceStream::new(
        Box::new(Cursor::new(bundle.ogg_opus_bytes)),
        MediaSourceStreamOptions::default(),
    );
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .unwrap();

    let mut format = probed.format;
    let track = format.default_track().unwrap();
    let track_id = track.id;
    let params = track.codec_params.clone();

    assert_eq!(params.codec, CODEC_TYPE_OPUS);
    assert_eq!(params.sample_rate, Some(OPUS_SAMPLE_RATE_HZ));
    assert_eq!(params.channels.map(Channels::count), Some(1));
    assert_eq!(params.delay, Some(312));

    let mut opus_decoder = opus::Decoder::new(OPUS_SAMPLE_RATE_HZ, opus::Channels::Mono).unwrap();
    let mut packet_pcm = vec![0.0_f32; 5_760];
    let mut decoded_48k = Vec::new();

    loop {
        match format.next_packet() {
            Ok(pkt) => {
                if pkt.track_id() != track_id {
                    continue;
                }
                let frames = opus_decoder
                    .decode_float(pkt.buf(), &mut packet_pcm, false)
                    .unwrap();
                decoded_48k.extend_from_slice(&packet_pcm[..frames]);
            }
            Err(SymphoniaError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => break,
            Err(other) => panic!("unexpected Symphonia Ogg demux error: {other}"),
        }
    }

    let pre_skip = usize::try_from(params.delay.unwrap_or(0)).unwrap();
    assert!(decoded_48k.len() > pre_skip);
    decoded_48k.drain(..pre_skip);
    if let Some(n_frames) = params.n_frames.and_then(|n| usize::try_from(n).ok())
        && n_frames < decoded_48k.len()
    {
        decoded_48k.truncate(n_frames);
    }

    let count_u128 = u128::try_from(decoded_48k.len()).unwrap();
    let decoded_duration_ms =
        u32::try_from((count_u128 * 1_000) / u128::from(OPUS_SAMPLE_RATE_HZ)).unwrap();
    assert!(
        decoded_duration_ms.abs_diff(1_500) < 40,
        "expected < 40ms Ogg/Opus duration drift from 1500ms, got {decoded_duration_ms}ms"
    );

    let decoded_rms = compute_rms(&decoded_48k);
    assert!(
        decoded_rms > 0.10,
        "decoded Ogg/Opus audio must preserve speech signal energy (got RMS {decoded_rms})"
    );
}

#[test]
fn symphonia_and_hound_verify_encoded_48khz_16bit_mono_wav() {
    let codec = RealAudioCodecEngine::new();
    let bundle = codec.encode_bundle(&synthetic_24k_pcm(1_500)).unwrap();

    let mss = MediaSourceStream::new(
        Box::new(Cursor::new(bundle.wav_48k_bytes.clone())),
        MediaSourceStreamOptions::default(),
    );
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .unwrap();
    let track = probed.format.default_track().unwrap();
    assert_eq!(track.codec_params.sample_rate, Some(48_000));
    assert_eq!(track.codec_params.channels.map(Channels::count), Some(1));

    let mut wav_reader = hound::WavReader::new(Cursor::new(bundle.wav_48k_bytes)).unwrap();
    let spec = wav_reader.spec();
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.sample_rate, 48_000);
    assert_eq!(spec.bits_per_sample, 16);

    let samples: Vec<f32> = wav_reader
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / f32::from(i16::MAX))
        .collect();
    assert_eq!(samples.len(), 72_000);
    assert!(compute_rms(&samples) > 0.15);
}

#[test]
fn decode_and_measure_enforces_gate0_across_speech_short_silent_clipped_and_opus_roundtrip() {
    let codec = RealAudioCodecEngine::new();

    // 1. Nominal 1_600 ms voiced WAV passes Gate 0
    let (speech_pcm, speech_metrics) = codec.decode_and_measure(&synthetic_speech_wav()).unwrap();
    assert_eq!(evaluate_audio_gate(&speech_metrics), Ok(()));
    assert!(speech_pcm.duration_ms().abs_diff(1_600) <= 40);
    assert_eq!(speech_metrics.clipped_samples_bps, BasisPoints::ZERO);

    // 2. Short 300 ms voiced WAV fails Gate 0 with TooShortVoiced
    let (_, short_metrics) = codec
        .decode_and_measure(&synthetic_speech_wav_ms(300))
        .unwrap();
    assert!(matches!(
        evaluate_audio_gate(&short_metrics),
        Err(AudioDomainError::TooShortVoiced { .. })
    ));

    // 3. Silent 250 ms WAV yields valid non-empty Pcm16kMono and fails Gate 0 with SilenceDetected
    let (silent_pcm, silent_metrics) = codec
        .decode_and_measure(&synthetic_silent_short_wav())
        .unwrap();
    assert_ne!(silent_pcm.samples(), &[] as &[f32]);
    assert_eq!(silent_metrics.voiced_duration_ms, 0);
    assert_eq!(silent_metrics.peak_amplitude_bps, BasisPoints::ZERO);
    assert!(matches!(
        evaluate_audio_gate(&silent_metrics),
        Err(AudioDomainError::SilenceDetected { .. })
    ));

    // 4. Clipped 1_500 ms WAV (> 150 bps clipped) fails Gate 0 with ExcessiveClipping
    let (_, clipped_metrics) = codec.decode_and_measure(&synthetic_clipped_wav()).unwrap();
    assert!(clipped_metrics.clipped_samples_bps.value() > 150);
    assert!(matches!(
        evaluate_audio_gate(&clipped_metrics),
        Err(AudioDomainError::ExcessiveClipping { .. })
    ));

    // 5. Full Ogg/Opus -> decode_and_measure round-trip passes Gate 0 with < 40 ms drift
    let bundle = codec.encode_bundle(&synthetic_24k_pcm(1_500)).unwrap();
    let (roundtrip_pcm_16k, roundtrip_metrics) =
        codec.decode_and_measure(&bundle.ogg_opus_bytes).unwrap();
    assert_eq!(evaluate_audio_gate(&roundtrip_metrics), Ok(()));
    assert!(roundtrip_metrics.total_duration_ms.abs_diff(1_500) < 40);
    assert!(roundtrip_pcm_16k.duration_ms().abs_diff(1_500) < 60);
}
